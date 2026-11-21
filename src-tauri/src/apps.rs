//! Installed apps, how recently they were used, and the fully automatic unused-app cleaner.

use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use crate::changelog::{self, Change};
use crate::config::RemovedApp;
use crate::state::{cfg, core, lock, with_cfg};
use crate::toast;
use crate::win::{self, now, Hive};

const UNINSTALL: &[(Hive, &str)] = &[
    (Hive::HKLM, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    (Hive::HKLM, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
    (Hive::HKCU, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
];

/// Never touched by the cleaner, matched against name + publisher.
const PROTECTED: &[&str] = &[
    "microsoft", "windows", "nvidia", "amd ", "amd software", "radeon", "intel", "realtek", "driver", "runtime",
    "redistributable", ".net", "directx", "visual c++", "vc++", "webview", "steam", "epic games", "battle.net",
    "riot", "roblox", "ubisoft", "ea app", "gog galaxy", "xbox", "defender", "antivirus", "malwarebytes",
    "bitdefender", "norton", "kaspersky", "eset", "avast", "avg", "mcafee", "sophos", "nightmoth", "java",
    "python", "node.js", "git", "vulkan", "physx", "openal", "easyanticheat", "battleye", "vanguard",
    "synaptics", "elan", "dolby", "waves", "logitech", "razer", "corsair", "steelseries", "asus", "msi ",
    "dell", "hp ", "lenovo", "acer", "framework", "bluetooth", "wi-fi", "wifi", "network", "audio", "chipset",
    "bios", "firmware", "tpm", "vpn", "openvpn", "wireguard", "office", "teams", "onedrive", "edge",
];

#[derive(Serialize, Clone, Debug)]
pub struct InstalledApp {
    pub key: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub location: String,
    pub size_mb: u64,
    pub installed: Option<i64>,
    pub last_used: Option<i64>,
    pub days_unused: Option<i64>,
    pub protected: bool,
    pub kept: bool,
    pub silent: bool,
    pub candidate: bool,
    #[serde(skip)]
    pub uninstall: String,
    #[serde(skip)]
    pub quiet: Option<String>,
}

impl InstalledApp {
    /// Registered in HKCU (per-user install) rather than machine-wide.
    pub fn per_user(&self) -> bool {
        self.key.starts_with("HKCU")
    }
}

fn parse_date(s: &str) -> Option<i64> {
    let d = chrono::NaiveDate::parse_from_str(s.trim(), "%Y%m%d").ok()?;
    Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

fn mtime(p: &Path) -> Option<i64> {
    let m = std::fs::metadata(p).ok()?;
    let t = m.modified().ok()?;
    Some(t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs() as i64)
}

fn ctime(p: &Path) -> Option<i64> {
    let m = std::fs::metadata(p).ok()?;
    let t = m.created().ok()?;
    Some(t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs() as i64)
}

/// exe name (lowercase) -> last run time, from Windows Prefetch.
fn prefetch_map() -> HashMap<String, i64> {
    let mut map = HashMap::new();
    let dir = Path::new(&std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into())).join("Prefetch");
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let n = e.file_name().to_string_lossy().to_lowercase();
            if !n.ends_with(".pf") {
                continue;
            }
            if let Some(idx) = n.rfind('-') {
                let exe = n[..idx].to_string();
                if let Some(t) = mtime(&e.path()) {
                    let entry = map.entry(exe).or_insert(0);
                    if t > *entry {
                        *entry = t;
                    }
                }
            }
        }
    }
    map
}

/// exe files inside an install folder (2 levels deep, bounded).
fn exes_in(dir: &Path) -> Vec<String> {
    let mut out = vec![];
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                if depth < 2 {
                    stack.push((p, depth + 1));
                }
            } else if p.extension().map(|x| x.eq_ignore_ascii_case("exe")).unwrap_or(false) {
                out.push(e.file_name().to_string_lossy().to_lowercase());
            }
            if out.len() > 300 {
                return out;
            }
        }
    }
    out
}

/// File name of the program a command line starts (lowercase).
fn program_name(cmd: &str) -> String {
    let c = cmd.trim();
    let path = if let Some(rest) = c.strip_prefix('"') {
        rest.split('"').next().unwrap_or("")
    } else {
        let low = c.to_lowercase();
        match low.find(".exe") {
            Some(i) => &c[..i + 4],
            None => c.split(' ').next().unwrap_or(""),
        }
    };
    Path::new(path).file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn quiet_command(uninstall: &str, quiet: Option<&str>) -> Option<String> {
    if let Some(q) = quiet {
        if !q.trim().is_empty() {
            return Some(q.to_string());
        }
    }
    let u = uninstall.trim();
    let low = u.to_lowercase();
    if low.contains("msiexec") {
        let start = u.find('{')?;
        let end = u[start..].find('}')? + start;
        return Some(format!("MsiExec.exe /X{} /qn /norestart", &u[start..=end]));
    }
    let prog = program_name(u);
    if prog.starts_with("unins0") {
        // Inno Setup (unins000.exe)
        return Some(format!("{} /VERYSILENT /SUPPRESSMSGBOXES /NORESTART", u));
    }
    if prog.starts_with("uninst") && prog.ends_with(".exe") {
        // NSIS / electron-builder (uninstall.exe, uninst.exe, "Uninstall Foo.exe")
        return Some(if low.contains(" /s") { u.to_string() } else { format!("{} /S", u) });
    }
    if prog == "update.exe" && low.contains("--uninstall") {
        // Squirrel (Discord, Slack, GitHub Desktop…)
        return Some(if low.contains(" -s") { u.to_string() } else { format!("{} -s", u) });
    }
    None
}

fn filetime_to_unix(ft: u64) -> Option<i64> {
    if ft == 0 {
        return None;
    }
    let t = (ft / 10_000_000) as i64 - 11_644_473_600;
    (946_684_800..now() + 86_400).contains(&t).then_some(t)
}

fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' => (((c as u8 - b'a' + 13) % 26) + b'a') as char,
            'A'..='Z' => (((c as u8 - b'A' + 13) % 26) + b'A') as char,
            _ => c,
        })
        .collect()
}

fn bump(map: &mut HashMap<String, i64>, k: String, t: i64) {
    let e = map.entry(k).or_insert(0);
    if t > *e {
        *e = t;
    }
}

/// Windows' own record of what you run (kept long before Nightmoth was installed):
/// exe name -> last run, and Start-menu/desktop shortcut name -> last run.
fn userassist() -> (HashMap<String, i64>, Vec<(String, i64)>) {
    const BASE: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist";
    let mut exes = HashMap::new();
    let mut lnks: HashMap<String, i64> = HashMap::new();
    for guid in win::reg_subkeys(Hive::HKCU, BASE) {
        for (name, data) in win::reg_values_raw(Hive::HKCU, &format!(r"{BASE}\{guid}\Count")) {
            if data.len() < 68 {
                continue;
            }
            let ft = u64::from_le_bytes([data[60], data[61], data[62], data[63], data[64], data[65], data[66], data[67]]);
            let Some(t) = filetime_to_unix(ft) else { continue };
            let path = rot13(&name).to_lowercase();
            let file = path.rsplit(['\\', '/']).next().unwrap_or("").to_string();
            if file.ends_with(".exe") {
                bump(&mut exes, file, t);
            } else if let Some(stem) = file.strip_suffix(".lnk") {
                bump(&mut lnks, stem.to_string(), t);
            }
        }
    }
    (exes, lnks.into_iter().collect())
}

/// Background Activity Moderator: last run time of every exe, per user (admin-only key).
fn bam() -> HashMap<String, i64> {
    let mut map = HashMap::new();
    for root in [r"SYSTEM\CurrentControlSet\Services\bam\State\UserSettings", r"SYSTEM\CurrentControlSet\Services\bam\UserSettings"] {
        for sid in win::reg_subkeys(Hive::HKLM, root) {
            for (name, data) in win::reg_values_raw(Hive::HKLM, &format!(r"{root}\{sid}")) {
                if data.len() < 8 {
                    continue;
                }
                let ft = u64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]);
                let Some(t) = filetime_to_unix(ft) else { continue };
                let file = name.rsplit('\\').next().unwrap_or("").to_lowercase();
                if file.ends_with(".exe") {
                    bump(&mut map, file, t);
                }
            }
        }
    }
    map
}

/// Last time a shortcut named like the app was opened.
fn shortcut_used(name: &str, lnks: &[(String, i64)]) -> Option<i64> {
    let n = name.to_lowercase();
    lnks.iter()
        .filter(|(stem, _)| stem.len() >= 4 && (n == *stem || n.starts_with(stem.as_str()) || stem.starts_with(n.as_str())))
        .map(|(_, t)| *t)
        .max()
}

fn is_protected(name: &str, publisher: &str) -> bool {
    let s = format!(" {} {} ", name.to_lowercase(), publisher.to_lowercase());
    PROTECTED.iter().any(|p| s.contains(p))
}

pub fn list() -> Vec<InstalledApp> {
    let c = cfg();
    // usage history from Windows itself, so apps unused since before Nightmoth was installed are found too
    let pf = prefetch_map();
    let (ua_exes, ua_lnks) = userassist();
    let bam = bam();
    let usage_works = !pf.is_empty() || !ua_exes.is_empty() || !bam.is_empty();
    let t = now();
    let mut seen = std::collections::HashSet::new();
    let mut out = vec![];
    for (hive, base) in UNINSTALL {
        for sub in win::reg_subkeys(*hive, base) {
            let path = format!(r"{base}\{sub}");
            let Some(name) = win::reg_str(*hive, &path, "DisplayName") else { continue };
            if name.trim().is_empty() {
                continue;
            }
            if win::reg_dword(*hive, &path, "SystemComponent") == Some(1) {
                continue;
            }
            if win::reg_str(*hive, &path, "ParentKeyName").is_some() {
                continue;
            }
            let rt = win::reg_str(*hive, &path, "ReleaseType").unwrap_or_default().to_lowercase();
            if rt.contains("update") || rt.contains("hotfix") {
                continue;
            }
            if !seen.insert(name.to_lowercase()) {
                continue;
            }
            let uninstall = win::reg_str(*hive, &path, "UninstallString").unwrap_or_default();
            if uninstall.is_empty() {
                continue;
            }
            let publisher = win::reg_str(*hive, &path, "Publisher").unwrap_or_default();
            let location = win::reg_str(*hive, &path, "InstallLocation").unwrap_or_default().trim_matches('"').to_string();
            let icon = win::reg_str(*hive, &path, "DisplayIcon").unwrap_or_default();
            let quiet = win::reg_str(*hive, &path, "QuietUninstallString");
            let size_mb = win::reg_dword(*hive, &path, "EstimatedSize").unwrap_or(0) as u64 / 1024;

            let mut installed = win::reg_str(*hive, &path, "InstallDate").and_then(|d| parse_date(&d));
            let loc_path = Path::new(&location);
            if installed.is_none() && !location.is_empty() {
                installed = ctime(loc_path);
            }

            // which exes belong to this app?
            let mut exes = if !location.is_empty() && loc_path.is_dir() { exes_in(loc_path) } else { vec![] };
            let icon_exe = icon.split(',').next().unwrap_or("").trim_matches('"').to_string();
            if icon_exe.to_lowercase().ends_with(".exe") {
                if let Some(f) = Path::new(&icon_exe).file_name() {
                    exes.push(f.to_string_lossy().to_lowercase());
                }
            }
            exes.retain(|e| !e.contains("unins") && !e.contains("setup") && !e.contains("crash") && !e.contains("update"));
            let lnk_used = shortcut_used(&name, &ua_lnks);
            let last_used = exes
                .iter()
                .flat_map(|e| [pf.get(e).copied(), ua_exes.get(e).copied(), bam.get(e).copied()])
                .flatten()
                .chain(lnk_used)
                .max();

            let reference = match (last_used, installed) {
                (Some(l), Some(i)) => Some(l.max(i)),
                (Some(l), None) => Some(l),
                (None, Some(i)) => Some(i),
                (None, None) => None,
            };
            let days_unused = reference.map(|r| (t - r) / 86400);
            let protected = is_protected(&name, &publisher);
            let kept = c.is_kept(&name);
            let silent = quiet_command(&uninstall, quiet.as_deref()).is_some();
            let installed_long_ago = installed.map(|i| t - i > 30 * 86400).unwrap_or(false);
            let candidate = usage_works
                && !protected
                && !kept
                && installed_long_ago
                && (!exes.is_empty() || lnk_used.is_some())
                && days_unused.map(|d| d >= c.unused_days as i64).unwrap_or(false);

            out.push(InstalledApp {
                key: format!("{:?}:{sub}", hive),
                name,
                publisher,
                version: win::reg_str(*hive, &path, "DisplayVersion").unwrap_or_default(),
                location,
                size_mb,
                installed,
                last_used,
                days_unused,
                protected,
                kept,
                silent,
                candidate,
                uninstall,
                quiet,
            });
        }
    }
    out.sort_by(|a, b| b.days_unused.unwrap_or(-1).cmp(&a.days_unused.unwrap_or(-1)));
    out
}

fn still_installed(name: &str) -> bool {
    UNINSTALL.iter().any(|(h, base)| {
        win::reg_subkeys(*h, base).iter().any(|sub| {
            win::reg_str(*h, &format!(r"{base}\{sub}"), "DisplayName").map(|n| n == name).unwrap_or(false)
        })
    })
}

/// Every way to uninstall an app silently, best first.
fn attempts(app: &InstalledApp) -> Vec<String> {
    let mut v: Vec<String> = vec![];
    if let Some(q) = quiet_command(&app.uninstall, app.quiet.as_deref()) {
        v.push(q);
    }
    // MSI product code from the uninstall string or the registry key name
    let sub = app.key.split_once(':').map(|x| x.1).unwrap_or("");
    let guid = [app.uninstall.as_str(), sub].iter().find_map(|t| {
        let a = t.find('{')?;
        let b = t[a..].find('}')? + a;
        Some(t[a..=b].to_string())
    });
    if let Some(g) = guid {
        v.push(format!("MsiExec.exe /X{g} /qn /norestart"));
    }
    let u = app.uninstall.trim();
    if !u.is_empty() && !u.to_lowercase().contains("msiexec") {
        for sw in ["/S", "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART", "/quiet /norestart", "/silent", "-s", "--silent", "--uninstall -s", "/uninstall /quiet", "-uninstall -silent", "/qn"] {
            v.push(format!("{u} {sw}"));
        }
    }
    let mut seen = std::collections::HashSet::new();
    v.retain(|x| seen.insert(x.to_lowercase()));
    v
}

fn run_attempt(app: &InstalledApp, line: &str, secs: u64) {
    if app.per_user() {
        // per-user entries could have been written by any program: never with admin rights
        let _ = crate::userjob::run("cmd", line, Some(Duration::from_secs(secs + 30)));
    } else {
        let _ = win::run_cmdline_timeout(line, secs);
    }
}

fn gone_within(name: &str, secs: u64) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < secs {
        if !still_installed(name) {
            return true;
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    !still_installed(name)
}

/// Last resort: stop the app, delete its own folder, its shortcuts and its uninstall entry.
/// Only for a dedicated install folder (never shared or system folders).
fn force_remove(app: &InstalledApp) -> Result<(), String> {
    let dir = leftover_dir(app).ok_or("its install folder isn't safe to delete by hand")?;
    let low = dir.to_string_lossy().to_lowercase();
    // close anything running from that folder
    {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        for p in sys.processes().values() {
            if p.exe().map(|e| e.to_string_lossy().to_lowercase().starts_with(&low)).unwrap_or(false) {
                win::kill_process(p.pid().as_u32());
            }
        }
    }
    let sub = app.key.split_once(':').map(|x| x.1.to_string()).unwrap_or_default();
    if app.per_user() {
        let _ = crate::userjob::run("cmd", &format!("rmdir /s /q \"{}\"", dir.to_string_lossy()), Some(Duration::from_secs(180)));
        if !sub.is_empty() {
            let _ = crate::userjob::run("cmd", &format!("reg delete \"HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{sub}\" /f"), Some(Duration::from_secs(60)));
        }
    } else {
        win::force_delete(&dir);
        for (h, base) in UNINSTALL.iter().filter(|(h, _)| *h == Hive::HKLM) {
            let k = format!(r"{base}\{sub}");
            if !sub.is_empty() && win::reg_str(*h, &k, "DisplayName").as_deref() == Some(app.name.as_str()) {
                let _ = win::reg_delete_tree(*h, &k);
            }
        }
    }
    remove_shortcuts_into(&low);
    Ok(())
}

/// Delete Start menu and desktop shortcuts that point into a folder.
fn remove_shortcuts_into(dir_low: &str) {
    let mut roots = vec![
        std::path::PathBuf::from(win::env("ProgramData")).join(r"Microsoft\Windows\Start Menu\Programs"),
        std::path::PathBuf::from(win::env("APPDATA")).join(r"Microsoft\Windows\Start Menu\Programs"),
        crate::organize::public_desktop(),
    ];
    roots.push(crate::tidy::desktop());
    for r in roots {
        let mut stack = vec![(r, 0)];
        while let Some((d, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.filter_map(|e| e.ok()) {
                let p = e.path();
                if p.is_dir() && depth < 3 {
                    stack.push((p, depth + 1));
                } else if p.extension().map(|x| x.eq_ignore_ascii_case("lnk")).unwrap_or(false) {
                    let (target, _) = crate::organize::target_of(&p);
                    if !target.is_empty() && target.to_lowercase().starts_with(dir_low) {
                        let _ = std::fs::remove_file(&p);
                    }
                }
            }
            // drop the Start menu folder if it's empty now
            let _ = std::fs::remove_dir(&d);
        }
    }
}

/// Uninstall whatever it takes: the app's own silent uninstaller, the MSI product code,
/// winget, every common silent switch, and finally removing it by hand.
pub fn uninstall(app: &InstalledApp, module: &str) -> Result<(), String> {
    let mut how = String::new();
    for (i, line) in attempts(app).into_iter().enumerate() {
        // the app's own/known silent commands get time; guessed switches fail fast
        let (limit, wait) = if i < 2 { (5 * 60, 60) } else { (75, 15) };
        run_attempt(app, &line, limit);
        if gone_within(&app.name, wait) {
            how = "uninstaller".into();
            break;
        }
    }
    if how.is_empty() {
        let w = format!(
            "winget uninstall --name \"{}\" --exact --silent --force --accept-source-agreements --disable-interactivity",
            app.name.replace('"', "")
        );
        run_attempt(app, &w, 5 * 60);
        if gone_within(&app.name, 60) {
            how = "winget".into();
        }
    }
    let mut forced = false;
    if how.is_empty() {
        force_remove(app).map_err(|e| format!("{} couldn't be removed: no uninstaller worked and {e}", app.name))?;
        if !gone_within(&app.name, 10) {
            return Err(format!("{} is still installed", app.name));
        }
        forced = true;
    }
    let leftovers = !forced && remove_leftovers(app);
    let detail = match (forced, leftovers) {
        (true, _) => format!("{} MB · removed by hand (its uninstaller didn't work)", app.size_mb),
        (false, true) => format!("{} MB · leftover folder deleted", app.size_mb),
        _ => format!("{} MB", app.size_mb),
    };
    changelog::record(module, &format!("Removed {}", app.name), &detail, Change::AppRemoved { name: app.name.clone() });
    with_cfg(|cf| {
        cf.stats.apps_removed += 1;
        cf.stats.freed_bytes += app.size_mb * 1024 * 1024;
        cf.removed_apps.push(RemovedApp {
            name: app.name.clone(),
            date: now(),
            size_mb: app.size_mb,
            days_unused: app.days_unused.unwrap_or(0),
            reinstalled: false,
        });
    });
    Ok(())
}

/// Is this install folder safe to delete after uninstalling? Only a dedicated app folder
/// inside the usual install locations, never a shared or system folder, and never one
/// another installed app still lives in.
fn leftover_dir(app: &InstalledApp) -> Option<std::path::PathBuf> {
    let loc = app.location.trim().trim_end_matches('\\');
    if loc.is_empty() {
        return None;
    }
    let p = Path::new(loc);
    let meta = std::fs::symlink_metadata(p).ok()?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return None;
    }
    let low = loc.to_lowercase();
    let mut roots: Vec<String> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"].iter().map(|v| win::env(v)).collect();
    if app.per_user() {
        let local = win::env("LOCALAPPDATA");
        roots = vec![format!(r"{local}\Programs"), local, win::env("APPDATA")];
    }
    let inside = roots.iter().filter(|r| !r.is_empty()).any(|r| {
        let r = r.to_lowercase();
        low.starts_with(&format!("{r}\\")) && !low[r.len() + 1..].is_empty()
    });
    if !inside {
        return None;
    }
    let last = p.file_name()?.to_string_lossy().to_lowercase();
    const SHARED: &[&str] = &["common files", "windowsapps", "microsoft", "windows", "programs", "temp", "packages", "microsoft shared", "internet explorer", "windows defender"];
    if SHARED.contains(&last.as_str()) {
        return None;
    }
    // another app still installed in (or above) this folder?
    let others = list();
    if others.iter().any(|o| {
        let ol = o.location.trim().trim_end_matches('\\').to_lowercase();
        !ol.is_empty() && o.name != app.name && (ol == low || ol.starts_with(&format!("{low}\\")) || low.starts_with(&format!("{ol}\\")))
    }) {
        return None;
    }
    Some(p.to_path_buf())
}

/// Delete what the uninstaller left behind. Per-user folders are deleted with the user's rights.
fn remove_leftovers(app: &InstalledApp) -> bool {
    let Some(dir) = leftover_dir(app) else { return false };
    if app.per_user() {
        let line = format!("rmdir /s /q \"{}\"", dir.to_string_lossy());
        let _ = crate::userjob::run("cmd", &line, Some(Duration::from_secs(120)));
    } else {
        // remove_dir_all never follows junctions or symlinks
        let _ = std::fs::remove_dir_all(&dir);
    }
    !dir.exists()
}

pub fn uninstall_by_key(key: &str) -> Result<String, String> {
    let app = list().into_iter().find(|a| a.key == key).ok_or("App not found")?;
    if app.protected {
        return Err(format!("{} is protected", app.name));
    }
    uninstall(&app, "Apps")?;
    toast::show("remove", &format!("Removed {}", app.name), &format!("{} MB freed", app.size_mb));
    Ok(app.name)
}

/// Try to reinstall an app by name through winget.
pub fn reinstall(name: &str) -> Result<(), String> {
    let out = win::run(
        "winget.exe",
        &["install", "--name", name, "--exact", "--silent", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"],
    );
    match out {
        Ok(_) => {
            with_cfg(|cf| {
                for r in cf.removed_apps.iter_mut().filter(|r| r.name == name) {
                    r.reinstalled = true;
                }
            });
            toast::show("remove", "Reinstalled", name);
            Ok(())
        }
        Err(_) => Err(format!("winget couldn't find \"{name}\" — reinstall it from its website")),
    }
}

/// Autopilot: remove apps unused for longer than the threshold, each with a 5 s cancel window.
pub fn unused_tick() {
    let c = cfg();
    if !c.unused_cleaner {
        return;
    }
    if now() - c.last_unused_check < 24 * 3600 {
        return;
    }
    // give the PC time to settle after boot
    if now() - core().started < 10 * 60 {
        return;
    }
    if core().boosting.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    with_cfg(|cf| cf.last_unused_check = now());

    let app_up = crate::userjob::app_connected();
    let candidates: Vec<InstalledApp> = list().into_iter().filter(|a| a.candidate && (app_up || !a.per_user())).take(8).collect();
    if candidates.is_empty() {
        return;
    }
    let _ = crate::speed::restore_point(false);
    for app in candidates {
        let id_cell = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let name = app.name.clone();
        let cell = id_cell.clone();
        let tid = toast::show_action(
            "remove",
            &format!("Removing {} in 5s", app.name),
            &format!("Unused for {} days · {} MB", app.days_unused.unwrap_or(0), app.size_mb),
            "Cancel",
            Box::new(move || {
                let id = lock(&cell).clone();
                lock(&core().cancelled).insert(id);
                with_cfg(|cf| {
                    if !cf.keep_list.contains(&name) {
                        cf.keep_list.push(name.clone());
                    }
                });
                toast::show("remove", "Kept", &format!("{name} won't be removed again"));
            }),
        );
        *lock(&id_cell) = tid.clone();
        // the countdown starts when the toast appears (it may wait behind others)
        // never remove anything the user didn't get the chance to see and cancel
        if !toast::wait_shown(&tid, Duration::from_secs(120)) {
            crate::log::log(&format!("skipped removing {}: the warning pop-up wasn't shown", app.name));
            with_cfg(|cf| cf.last_unused_check = 0);
            return;
        }
        std::thread::sleep(Duration::from_millis(c.toast_ms.clamp(2000, 15000) + 800));
        if lock(&core().cancelled).remove(&tid) {
            continue;
        }
        match uninstall(&app, "Autopilot") {
            Ok(_) => toast::show("remove", &format!("Removed {}", app.name), &format!("{} MB freed", app.size_mb)),
            Err(e) => toast::show("remove", &format!("Couldn't remove {}", app.name), &e),
        }
    }
}
