//! Deep cleanup: the big, rarely-cleaned stuff. Deleted straight away (by choice).
//!
//! - old restore points (the newest is always kept)
//! - superseded Windows components (DISM component cleanup)
//! - old driver versions (the newest of each is kept; drivers in use are never removed)
//! - folders, settings and registry keys left behind by apps that were uninstalled
//! - GPU driver installer leftovers (C:\NVIDIA, C:\AMD, old driver downloads)
//! - Windows.old after the 10-day rollback window
//! - memory dumps, old Windows setup/servicing logs
//! - launcher web caches (Discord, Steam, Epic) while they're closed
//! - installers in Downloads older than 60 days

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::state::{cfg, with_cfg};
use crate::toast;
use crate::win::{self, Hive};

#[derive(Serialize, Clone, Debug)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub bytes: u64,
}

fn env_path(v: &str) -> PathBuf {
    PathBuf::from(win::env(v))
}

fn sysdrive() -> PathBuf {
    let d = win::env("SystemDrive");
    PathBuf::from(if d.is_empty() { "C:".to_string() } else { d } + "\\")
}

fn real_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map(|m| m.is_dir() && !m.file_type().is_symlink()).unwrap_or(false)
}

fn size_of(p: &Path) -> u64 {
    let Ok(m) = std::fs::symlink_metadata(p) else { return 0 };
    if m.file_type().is_symlink() {
        return 0;
    }
    if m.is_file() {
        return m.len();
    }
    let mut total = 0;
    let mut stack = vec![p.to_path_buf()];
    let mut seen = 0u32;
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            seen += 1;
            if seen > 400_000 {
                return total;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(e.path());
            } else if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    total
}

fn older_than(p: &Path, days: u64) -> bool {
    std::fs::symlink_metadata(p)
        .and_then(|m| m.modified())
        .map(|t| SystemTime::now().duration_since(t).unwrap_or(Duration::ZERO) > Duration::from_secs(days * 86400))
        .unwrap_or(false)
}

fn remove(p: &Path) -> u64 {
    let size = size_of(p);
    let ok = match std::fs::symlink_metadata(p) {
        Ok(m) if m.file_type().is_symlink() => false,
        // every way there is: normal delete, then flags/ownership, then at the next restart
        Ok(_) => win::force_delete(p) && !p.exists(),
        Err(_) => false,
    };
    if ok {
        size
    } else {
        size.saturating_sub(size_of(p))
    }
}

fn running(exe: &str) -> bool {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.processes().values().any(|p| p.name().to_string_lossy().eq_ignore_ascii_case(exe))
}

// ------------------------------------------------------------------ restore points

fn shadow_count() -> usize {
    let drive = win::env("SystemDrive");
    win::run("vssadmin.exe", &["list", "shadows", &format!("/for={drive}")])
        .map(|o| o.lines().filter(|l| l.to_lowercase().contains("shadow copy id")).count())
        .unwrap_or(0)
}

fn old_restore_points() -> u64 {
    let n = shadow_count();
    if n <= 1 {
        return 0;
    }
    let (before, _) = win::system_drive_space();
    let drive = win::env("SystemDrive");
    for _ in 0..n - 1 {
        if win::run("vssadmin.exe", &["delete", "shadows", &format!("/for={drive}"), "/oldest", "/quiet"]).is_err() {
            break;
        }
    }
    let (after, _) = win::system_drive_space();
    after.saturating_sub(before)
}

// ------------------------------------------------------------------ components & drivers

fn component_cleanup() -> u64 {
    let (before, _) = win::system_drive_space();
    let _ = win::run("Dism.exe", &["/Online", "/Cleanup-Image", "/StartComponentCleanup", "/Quiet", "/NoRestart"]);
    let (after, _) = win::system_drive_space();
    after.saturating_sub(before)
}

fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.trim().parse::<u64>().unwrap_or(0)).collect()
}

/// Old versions of the same driver package. Returns (oem inf, size).
fn old_drivers() -> Vec<(String, String, u64)> {
    let list = win::ps_json(
        "Get-WindowsDriver -Online -ErrorAction SilentlyContinue | Select-Object Driver,OriginalFileName,ProviderName,ClassName,Version | ConvertTo-Json -Compress",
    );
    let mut groups: std::collections::HashMap<String, Vec<(String, String, Vec<u64>, PathBuf)>> = Default::default();
    for d in list {
        let (Some(oem), Some(orig), Some(ver)) = (d["Driver"].as_str(), d["OriginalFileName"].as_str(), d["Version"].as_str()) else { continue };
        let orig_path = PathBuf::from(orig);
        let file = orig_path.file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
        let key = format!("{}|{}|{}", file, d["ProviderName"].as_str().unwrap_or(""), d["ClassName"].as_str().unwrap_or(""));
        let folder = orig_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        groups.entry(key).or_default().push((oem.to_string(), file, version_key(ver), folder));
    }
    let mut out = vec![];
    for (_, mut v) in groups {
        if v.len() < 2 {
            continue;
        }
        v.sort_by(|a, b| b.2.cmp(&a.2));
        for (oem, file, _, folder) in v.into_iter().skip(1) {
            out.push((oem, file, size_of(&folder)));
        }
    }
    out
}

fn remove_old_drivers() -> u64 {
    let mut freed = 0;
    for (oem, _, size) in old_drivers() {
        // no /force: Windows refuses to remove a driver that's still in use
        if win::run("pnputil.exe", &["/delete-driver", &oem]).is_ok() {
            freed += size;
        }
    }
    freed
}

// ------------------------------------------------------------------ app leftovers

const NEVER: &[&str] = &[
    "microsoft", "windows", "packages", "temp", "programs", "nvidia", "amd", "intel", "google", "mozilla", "nightmoth",
    "steam", "epic games", "riot games", "roblox", "discord", "common files", "package cache", "regid.1991-06.com.microsoft",
    "ssh", "git", "npm", "nuget", "pip", "python", "java", "oracle", "apple", "adobe", "realtek", "dell", "hp", "lenovo",
    "asus", "msi", "logitech", "razer", "corsair", "elgato", "obs-studio", "connecteddevicesplatform", "comms", "d3dscache",
    "crashdumps", "virtualstore", "history", "identities", "fonts", "assembly", "desktop", "documents", "downloads",
];

/// Keys an app might use for its folders: its name without version/bitness, and its publisher.
fn app_keys(name: &str, publisher: &str) -> Vec<String> {
    let mut clean = name.to_lowercase();
    for junk in ["(x64)", "(x86)", "(64-bit)", "(32-bit)", "64-bit", "32-bit", "x64", "x86"] {
        clean = clean.replace(junk, "");
    }
    let clean: String = clean
        .split_whitespace()
        .take_while(|w| {
            // stop at the version number: "7.2", "v3", "2024"
            let mut c = w.chars();
            let first = c.next().unwrap_or(' ');
            !(first.is_ascii_digit() || (first == 'v' && c.next().map(|d| d.is_ascii_digit()).unwrap_or(false)))
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut keys = vec![clean.trim().to_string()];
    let pubk = publisher
        .to_lowercase()
        .replace(", inc.", "")
        .replace(" inc.", "")
        .replace(" inc", "")
        .replace(" ltd", "")
        .replace(" llc", "")
        .replace(" gmbh", "")
        .replace(" ab", "")
        .replace(" corporation", "")
        .trim()
        .trim_end_matches('.')
        .to_string();
    if !pubk.is_empty() {
        keys.push(pubk);
    }
    keys.retain(|k| k.len() >= 3 && !NEVER.contains(&k.as_str()));
    keys.dedup();
    keys
}

/// Remember every installed app so leftovers can be recognised after it's gone.
pub fn remember_apps() -> Vec<(String, String)> {
    let installed: Vec<(String, String)> = crate::apps::list().into_iter().map(|a| (a.name, a.publisher)).collect();
    with_cfg(|c| {
        for (n, p) in &installed {
            let k = format!("{n}|{p}");
            if !c.seen_apps.contains(&k) {
                c.seen_apps.push(k);
            }
        }
        if c.seen_apps.len() > 3000 {
            c.seen_apps.drain(..500);
        }
    });
    installed
}

/// Folders and registry keys of apps that were installed before and are gone now.
fn leftovers() -> (Vec<PathBuf>, Vec<(Hive, String)>) {
    let installed = remember_apps();
    let installed_keys: Vec<String> = installed.iter().flat_map(|(n, p)| app_keys(n, p)).collect();
    let c = cfg();
    let mut gone: Vec<(String, String)> = c
        .seen_apps
        .iter()
        .filter_map(|k| k.split_once('|').map(|(n, p)| (n.to_string(), p.to_string())))
        .filter(|(n, _)| !installed.iter().any(|(i, _)| i == n))
        .collect();
    gone.extend(c.removed_apps.iter().map(|r| (r.name.clone(), String::new())));

    let roots = [env_path("APPDATA"), env_path("LOCALAPPDATA"), env_path("ProgramData"), env_path("LOCALAPPDATA").join("Programs")];
    let mut dirs = vec![];
    let mut keys = vec![];
    for (name, publisher) in &gone {
        let ks = app_keys(name, publisher);
        let Some(app_key) = ks.first() else { continue };
        // the folder must be named exactly like the app, and no installed app may claim that name
        if installed_keys.iter().any(|k| k == app_key || k.contains(app_key.as_str()) || app_key.contains(k.as_str())) {
            continue;
        }
        let pubk = ks.get(1).cloned();
        for root in &roots {
            let mut cands = vec![root.join(app_key)];
            if let Some(p) = &pubk {
                if !installed_keys.iter().any(|k| k == p) {
                    cands.push(root.join(p).join(app_key));
                }
            }
            for d in cands {
                if let Some(found) = find_ci(&d) {
                    if real_dir(&found) && older_than(&found, 7) && !dirs.contains(&found) {
                        dirs.push(found);
                    }
                }
            }
        }
        for (hive, base) in [(Hive::HKCU, "Software"), (Hive::HKLM, "SOFTWARE"), (Hive::HKLM, r"SOFTWARE\WOW6432Node")] {
            let mut cand = vec![format!(r"{base}\{app_key}")];
            if let Some(p) = &pubk {
                cand.push(format!(r"{base}\{p}\{app_key}"));
            }
            for k in cand {
                if let Some(real) = find_key_ci(hive, &k) {
                    if !keys.contains(&(hive, real.clone())) {
                        keys.push((hive, real));
                    }
                }
            }
        }
    }
    (dirs, keys)
}

/// Case-insensitive lookup of the last path component.
fn find_ci(p: &Path) -> Option<PathBuf> {
    let parent = p.parent()?;
    let want = p.file_name()?.to_string_lossy().to_lowercase();
    std::fs::read_dir(parent).ok()?.filter_map(|e| e.ok()).find(|e| e.file_name().to_string_lossy().to_lowercase() == want).map(|e| e.path())
}

fn find_key_ci(h: Hive, path: &str) -> Option<String> {
    let (parent, last) = path.rsplit_once('\\')?;
    win::reg_subkeys(h, parent).into_iter().find(|k| k.eq_ignore_ascii_case(last)).map(|k| format!(r"{parent}\{k}"))
}

// ------------------------------------------------------------------ the list

fn downloads() -> PathBuf {
    win::reg_str(Hive::HKCU, r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders", "{374DE290-123F-4565-9164-39C4925E467B}")
        .map(PathBuf::from)
        .unwrap_or_else(|| env_path("USERPROFILE").join("Downloads"))
}

fn old_installers() -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(downloads()) else { return vec![] };
    rd.filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            ["exe", "msi", "msix", "msixbundle", "appx"].contains(&ext.as_str()) && p.is_file() && older_than(p, 60)
        })
        .collect()
}

fn gpu_leftovers() -> Vec<PathBuf> {
    let mut v = vec![sysdrive().join("NVIDIA"), sysdrive().join("AMD")];
    v.push(env_path("ProgramData").join(r"NVIDIA Corporation\Downloader"));
    v.push(env_path("ProgramData").join(r"NVIDIA Corporation\NVIDIA App\Downloader"));
    v.retain(|p| real_dir(p));
    v
}

fn old_logs() -> Vec<PathBuf> {
    let windir = env_path("WINDIR");
    let mut v = vec![];
    for dir in [windir.join(r"Logs\CBS"), windir.join(r"Logs\DISM"), windir.join(r"Logs\WindowsUpdate"), windir.join("Panther")] {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.filter_map(|e| e.ok()) {
                let p = e.path();
                let n = e.file_name().to_string_lossy().to_lowercase();
                if p.is_file() && n != "cbs.log" && n != "dism.log" && older_than(&p, 7) {
                    v.push(p);
                }
            }
        }
    }
    let dump = windir.join("MEMORY.DMP");
    if dump.exists() {
        v.push(dump);
    }
    v
}

fn launcher_caches() -> Vec<PathBuf> {
    let mut v = vec![];
    if !running("Discord.exe") {
        for d in ["Cache", "Code Cache", "GPUCache"] {
            v.push(env_path("APPDATA").join("discord").join(d));
        }
    }
    if !running("steam.exe") && !running("steamwebhelper.exe") {
        v.push(env_path("LOCALAPPDATA").join(r"Steam\htmlcache"));
    }
    if !running("EpicGamesLauncher.exe") {
        if let Ok(rd) = std::fs::read_dir(env_path("LOCALAPPDATA").join(r"EpicGamesLauncher\Saved")) {
            for e in rd.filter_map(|e| e.ok()) {
                if e.file_name().to_string_lossy().to_lowercase().starts_with("webcache") {
                    v.push(e.path());
                }
            }
        }
    }
    v.retain(|p| real_dir(p));
    v
}

fn windows_old() -> Option<PathBuf> {
    let p = sysdrive().join("Windows.old");
    (real_dir(&p) && older_than(&p, 10)).then_some(p)
}

/// Shortcuts (Start menu, desktops) whose program is gone.
fn dead_shortcuts() -> Vec<PathBuf> {
    let roots = [
        env_path("ProgramData").join(r"Microsoft\Windows\Start Menu\Programs"),
        env_path("APPDATA").join(r"Microsoft\Windows\Start Menu\Programs"),
        crate::organize::public_desktop(),
        crate::tidy::desktop(),
    ];
    let mut out = vec![];
    for r in roots {
        let mut stack = vec![(r, 0)];
        while let Some((d, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.filter_map(|e| e.ok()) {
                let p = e.path();
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_dir() && !ft.is_symlink() && depth < 4 {
                    stack.push((p, depth + 1));
                } else if p.extension().map(|x| x.eq_ignore_ascii_case("lnk")).unwrap_or(false) {
                    let (target, _) = crate::organize::target_of(&p);
                    if let Some(exe) = crate::speed::command_exe(&format!("\"{target}\"")) {
                        if !target.is_empty() && !exe.exists() {
                            out.push(p);
                        }
                    }
                }
            }
        }
    }
    out
}

/// Broken startup entries and dead shortcuts. With `apply`, remove them (startup entries can be undone).
pub fn broken_things(apply: bool) -> (usize, usize) {
    let startup = crate::speed::broken_startup();
    let shortcuts = dead_shortcuts();
    if apply {
        let mut ch = vec![];
        for (h, key, name) in &startup {
            if let Ok(Some(c)) = crate::changelog::reg_del(*h, key, name) {
                ch.push(c);
            }
        }
        crate::changelog::record_many("Deep clean", "Removed broken startup entries", &format!("{} entries pointing at deleted programs", ch.len()), ch);
        for s in &shortcuts {
            let _ = std::fs::remove_file(s);
            if let Some(parent) = s.parent() {
                let _ = std::fs::remove_dir(parent); // empty Start menu folder
            }
        }
    }
    (startup.len(), shortcuts.len())
}

/// What a deep clean would remove (sizes where they're cheap to measure).
pub fn scan() -> Vec<Item> {
    let mut out = vec![];
    let item = |id: &str, name: &str, desc: String, bytes: u64| Item { id: id.into(), name: name.into(), desc, bytes };
    let n = shadow_count();
    out.push(item("restore", "Old restore points", if n > 1 { format!("{} old · the newest is kept", n - 1) } else { "Only the newest exists".into() }, 0));
    out.push(item("components", "Superseded Windows components", "Old versions kept after updates".into(), 0));
    let drivers = old_drivers();
    out.push(item("drivers", "Old driver versions", format!("{} old packages · drivers in use are never removed", drivers.len()), drivers.iter().map(|d| d.2).sum()));
    let (dirs, keys) = leftovers();
    out.push(item(
        "leftovers",
        "Leftovers from uninstalled apps",
        format!("{} folders, {} settings keys", dirs.len(), keys.len()),
        dirs.iter().map(|d| size_of(d)).sum(),
    ));
    let gpu = gpu_leftovers();
    out.push(item("gpu", "GPU driver installer leftovers", "C:\\NVIDIA, C:\\AMD, old driver downloads".into(), gpu.iter().map(|p| size_of(p)).sum()));
    if let Some(w) = windows_old() {
        out.push(item("windowsold", "Previous Windows install (Windows.old)", "The 10-day rollback window has passed".into(), size_of(&w)));
    }
    let logs = old_logs();
    out.push(item("logs", "Old Windows logs & memory dumps", format!("{} files", logs.len()), logs.iter().map(|p| size_of(p)).sum()));
    let (bs, ds) = broken_things(false);
    out.push(item("broken", "Broken startup entries & dead shortcuts", format!("{bs} startup entries and {ds} shortcuts pointing at deleted apps"), 0));
    let lc = launcher_caches();
    out.push(item("launchers", "Discord, Steam & Epic web caches", "Only while they're closed".into(), lc.iter().map(|p| size_of(p)).sum()));
    let inst = old_installers();
    out.push(item("installers", "Old installers in Downloads", format!("{} setup files older than 60 days", inst.len()), inst.iter().map(|p| size_of(p)).sum()));
    out
}

fn take_ownership_and_delete(p: &Path) -> u64 {
    let s = p.to_string_lossy().to_string();
    let size = size_of(p);
    let _ = win::run("takeown.exe", &["/F", &s, "/R", "/A", "/D", "Y"]);
    let _ = win::run("icacls.exe", &[&s, "/grant", "*S-1-5-32-544:F", "/T", "/C", "/Q"]);
    let _ = win::run_cmdline(&format!("rd /s /q \"{s}\""));
    if p.exists() {
        size.saturating_sub(size_of(p))
    } else {
        size
    }
}

/// Run the chosen parts. Returns (part name, bytes freed).
pub fn run(ids: &[String]) -> Vec<(String, u64)> {
    let want = |id: &str| ids.iter().any(|i| i == id);
    let mut report = vec![];
    if want("restore") {
        report.push(("Old restore points".to_string(), old_restore_points()));
    }
    if want("drivers") {
        report.push(("Old drivers".to_string(), remove_old_drivers()));
    }
    if want("leftovers") {
        let (dirs, keys) = leftovers();
        let freed: u64 = dirs.iter().map(|d| remove(d)).sum();
        for (h, k) in &keys {
            let _ = win::reg_delete_tree(*h, k);
        }
        if !dirs.is_empty() || !keys.is_empty() {
            crate::log::log(&format!("deep clean: removed {} leftover folders, {} keys", dirs.len(), keys.len()));
        }
        report.push(("App leftovers".to_string(), freed));
    }
    if want("gpu") {
        report.push(("GPU installer leftovers".to_string(), gpu_leftovers().iter().map(|p| remove(p)).sum()));
    }
    if want("logs") {
        report.push(("Old logs & dumps".to_string(), old_logs().iter().map(|p| remove(p)).sum()));
    }
    if want("broken") {
        broken_things(true);
    }
    if want("launchers") {
        report.push(("Launcher caches".to_string(), launcher_caches().iter().map(|p| remove(p)).sum()));
    }
    if want("installers") {
        report.push(("Old installers".to_string(), old_installers().iter().map(|p| remove(p)).sum()));
    }
    if want("windowsold") {
        if let Some(w) = windows_old() {
            report.push(("Windows.old".to_string(), take_ownership_and_delete(&w)));
        }
    }
    // slowest last
    if want("components") {
        report.push(("Windows components".to_string(), component_cleanup()));
    }
    let total: u64 = report.iter().map(|r| r.1).sum();
    with_cfg(|c| {
        c.stats.freed_bytes += total;
        c.last_deep_clean = crate::win::now();
        c.last_deep_report = report.clone();
    });
    crate::changelog::record("Deep clean", "Deep clean", &format!("Freed {}", crate::fmt_bytes(total)), crate::changelog::Change::Files { bytes: total });
    report
}

pub const ALL: &[&str] = &["restore", "drivers", "leftovers", "broken", "gpu", "logs", "launchers", "installers", "windowsold", "components"];

/// Autopilot (when you're away): everything.
pub fn run_auto() {
    let report = run(&ALL.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    let total: u64 = report.iter().map(|r| r.1).sum();
    if total > 0 {
        let top = report.iter().max_by_key(|r| r.1).map(|r| r.0.clone()).unwrap_or_default();
        toast::show("cleanup", "Deep clean done", &format!("Freed {} · most from {}", crate::fmt_bytes(total), top.to_lowercase()));
    }
}
