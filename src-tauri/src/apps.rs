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
    if low.contains("unins") && low.contains(".exe") {
        // Inno Setup
        return Some(format!("{} /VERYSILENT /SUPPRESSMSGBOXES /NORESTART", u));
    }
    if low.contains("uninstall.exe") || low.contains("uninst.exe") {
        // NSIS
        return Some(format!("{} /S", u));
    }
    None
}

fn is_protected(name: &str, publisher: &str) -> bool {
    let s = format!(" {} {} ", name.to_lowercase(), publisher.to_lowercase());
    PROTECTED.iter().any(|p| s.contains(p))
}

pub fn list() -> Vec<InstalledApp> {
    let c = cfg();
    let pf = prefetch_map();
    let prefetch_works = !pf.is_empty();
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
            let last_used = exes.iter().filter_map(|e| pf.get(e).copied()).max();

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
            let candidate = prefetch_works
                && !protected
                && !kept
                && silent
                && installed_long_ago
                && !exes.is_empty()
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

/// Uninstall silently. Returns Err if no silent method or it failed.
pub fn uninstall(app: &InstalledApp, module: &str) -> Result<(), String> {
    let cmd = quiet_command(&app.uninstall, app.quiet.as_deref()).ok_or("This app has no silent uninstaller — remove it from Windows Settings")?;
    win::run_cmdline(&cmd)?;
    // installers often finish in a child process; wait a little
    for _ in 0..40 {
        if !still_installed(&app.name) {
            break;
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    if still_installed(&app.name) {
        return Err(format!("{} is still installed", app.name));
    }
    changelog::record(module, &format!("Removed {}", app.name), &format!("{} MB", app.size_mb), Change::AppRemoved { name: app.name.clone() });
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

    let candidates: Vec<InstalledApp> = list().into_iter().filter(|a| a.candidate).take(5).collect();
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
        toast::wait_shown(&tid, Duration::from_secs(120));
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
