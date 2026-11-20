//! Real optimizations only: startup, junk cleanup, debloat, services, privacy, power.

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::changelog::{self, Change};
use crate::state::{cfg, with_cfg};
use crate::toast;
use crate::win::{self, env, now, ps, ps_json, ps_quote, Hive, RegVal};

// ================================================================== startup

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN32: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

#[derive(Serialize, Clone, Debug)]
pub struct StartupItem {
    pub id: String,
    pub name: String,
    pub command: String,
    pub location: String,
    pub kind: String,
    pub enabled: bool,
    pub impact: String,
}

const HEAVY: &[&str] = &[
    "teams", "discord", "steam", "spotify", "onedrive", "epic", "adobe", "creative cloud", "skype", "zoom",
    "dropbox", "googledrive", "battle.net", "riot", "ubisoft", "origin", "eadesktop", "slack", "edge", "opera",
    "wallpaper engine", "icue", "armoury", "razer", "logi", "msedge", "update",
];

fn impact(name: &str, cmd: &str) -> String {
    let s = format!("{} {}", name, cmd).to_lowercase();
    if HEAVY.iter().any(|h| s.contains(h)) {
        "High".into()
    } else if s.contains("tray") || s.contains("helper") || s.contains("agent") {
        "Low".into()
    } else {
        "Medium".into()
    }
}

fn approved_enabled(hive: Hive, sub: &str, name: &str) -> bool {
    match win::reg_get(hive, &format!(r"{APPROVED}\{sub}"), name) {
        Some(RegVal::Bin(b)) if !b.is_empty() => b[0] & 1 == 0,
        _ => true,
    }
}

struct RunSource {
    hive: Hive,
    key: &'static str,
    approved: &'static str,
    label: &'static str,
}

const RUN_SOURCES: &[RunSource] = &[
    RunSource { hive: Hive::HKCU, key: RUN, approved: "Run", label: "Registry (you)" },
    RunSource { hive: Hive::HKLM, key: RUN, approved: "Run", label: "Registry (all users)" },
    RunSource { hive: Hive::HKLM, key: RUN32, approved: "Run32", label: "Registry (all users, 32-bit)" },
];

fn startup_folders() -> Vec<(Hive, PathBuf, &'static str)> {
    vec![
        (Hive::HKCU, PathBuf::from(env("APPDATA")).join(r"Microsoft\Windows\Start Menu\Programs\Startup"), "Startup folder (you)"),
        (Hive::HKLM, PathBuf::from(env("ProgramData")).join(r"Microsoft\Windows\Start Menu\Programs\StartUp"), "Startup folder (all users)"),
    ]
}

pub fn startup_list(include_tasks: bool) -> Vec<StartupItem> {
    let mut out = vec![];
    for (si, src) in RUN_SOURCES.iter().enumerate() {
        for (name, cmd) in win::reg_values(src.hive, src.key) {
            if name.is_empty() {
                continue;
            }
            out.push(StartupItem {
                id: format!("run:{si}:{name}"),
                enabled: approved_enabled(src.hive, src.approved, &name),
                impact: impact(&name, &cmd),
                name,
                command: cmd,
                location: src.label.into(),
                kind: "run".into(),
            });
        }
    }
    for (fi, (hive, dir, label)) in startup_folders().into_iter().enumerate() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.filter_map(|e| e.ok()) {
                let file = e.file_name().to_string_lossy().to_string();
                if file.eq_ignore_ascii_case("desktop.ini") {
                    continue;
                }
                let name = Path::new(&file).file_stem().and_then(|s| s.to_str()).unwrap_or(&file).to_string();
                out.push(StartupItem {
                    id: format!("folder:{fi}:{file}"),
                    enabled: approved_enabled(hive, "StartupFolder", &file),
                    impact: impact(&name, ""),
                    name,
                    command: e.path().to_string_lossy().to_string(),
                    location: label.into(),
                    kind: "folder".into(),
                });
            }
        }
    }
    if include_tasks {
        let script = r#"$t = Get-ScheduledTask | Where-Object { $_.TaskPath -notlike '\Microsoft\*' -and $_.TaskName -ne 'Nightmoth' -and ($_.Triggers | Where-Object { $_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' -or $_.CimClass.CimClassName -eq 'MSFT_TaskBootTrigger' }) } | ForEach-Object { [pscustomobject]@{ n=$_.TaskName; p=$_.TaskPath; s=[string]$_.State; e=[string](($_.Actions | Select-Object -First 1).Execute) } }; ConvertTo-Json -InputObject @($t) -Compress"#;
        for v in ps_json(script) {
            let n = v["n"].as_str().unwrap_or("").to_string();
            let p = v["p"].as_str().unwrap_or("\\").to_string();
            let e = v["e"].as_str().unwrap_or("").to_string();
            if n.is_empty() {
                continue;
            }
            out.push(StartupItem {
                id: format!("task:{p}{n}"),
                enabled: !v["s"].as_str().unwrap_or("").eq_ignore_ascii_case("Disabled"),
                impact: impact(&n, &e),
                name: n,
                command: e,
                location: format!("Scheduled task {p}"),
                kind: "task".into(),
            });
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

pub fn set_task_enabled(path: &str, name: &str, enabled: bool) -> Result<(), String> {
    let verb = if enabled { "Enable-ScheduledTask" } else { "Disable-ScheduledTask" };
    ps(&format!("{verb} -TaskPath {} -TaskName {} | Out-Null", ps_quote(path), ps_quote(name))).map(|_| ())
}

fn approved_value(enabled: bool) -> RegVal {
    let mut b = vec![0u8; 12];
    b[0] = if enabled { 2 } else { 3 };
    RegVal::Bin(b)
}

/// Enable/disable a startup item the same way Task Manager does (reversible).
pub fn startup_set(id: &str, enabled: bool, module: &str) -> Result<String, String> {
    let parts: Vec<&str> = id.splitn(3, ':').collect();
    let (title, change) = match parts.as_slice() {
        ["run", si, name] => {
            let src = RUN_SOURCES.get(si.parse::<usize>().map_err(|_| "bad id")?).ok_or("bad id")?;
            let c = changelog::reg(src.hive, &format!(r"{APPROVED}\{}", src.approved), name, approved_value(enabled))?;
            (name.to_string(), c)
        }
        ["folder", fi, file] => {
            let folders = startup_folders();
            let (hive, _, _) = folders.get(fi.parse::<usize>().map_err(|_| "bad id")?).ok_or("bad id")?;
            let c = changelog::reg(*hive, &format!(r"{APPROVED}\StartupFolder"), file, approved_value(enabled))?;
            (file.to_string(), c)
        }
        _ if id.starts_with("task:") => {
            let full = &id[5..];
            let split = full.rfind('\\').map(|i| i + 1).unwrap_or(0);
            let (path, name) = full.split_at(split);
            let path = if path.is_empty() { "\\" } else { path };
            set_task_enabled(path, name, enabled)?;
            (
                name.to_string(),
                Some(Change::Task { path: path.into(), name: name.into(), enabled_before: !enabled }),
            )
        }
        _ => return Err("Unknown startup item".into()),
    };
    let verb = if enabled { "Enabled" } else { "Disabled" };
    if let Some(c) = change {
        changelog::record(module, &format!("{verb} startup: {title}"), "", c);
    }
    Ok(title)
}

/// Autopilot: block new startup entries that appeared since last check.
pub fn startup_watch() {
    let items = startup_list(false);
    let c = cfg();
    let ids: Vec<String> = items.iter().filter(|i| i.enabled).map(|i| i.id.clone()).collect();
    if !c.startup_seeded {
        with_cfg(|cf| {
            cf.known_startup = ids;
            cf.startup_seeded = true;
        });
        return;
    }
    for item in items.iter().filter(|i| i.enabled) {
        if c.known_startup.contains(&item.id) || c.startup_allow.contains(&item.id) {
            continue;
        }
        if startup_set(&item.id, false, "Autopilot").is_ok() {
            with_cfg(|cf| cf.stats.startup_blocked += 1);
            let id = item.id.clone();
            let name = item.name.clone();
            toast::show_action(
                "startup",
                "Startup blocked",
                &format!("{} tried to start with Windows", item.name),
                "Allow",
                Box::new(move || {
                    let _ = startup_set(&id, true, "Autopilot");
                    with_cfg(|cf| {
                        cf.startup_allow.push(id.clone());
                        cf.known_startup.push(id.clone());
                    });
                    toast::show("startup", "Allowed", &format!("{name} will start with Windows"));
                }),
            );
        }
    }
    let ids_now: Vec<String> = startup_list(false).iter().filter(|i| i.enabled).map(|i| i.id.clone()).collect();
    with_cfg(|cf| {
        for id in ids_now {
            if !cf.known_startup.contains(&id) {
                cf.known_startup.push(id);
            }
        }
    });
}

// ================================================================== cleanup

#[derive(Serialize, Clone, Debug)]
pub struct CleanTarget {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub bytes: u64,
}

fn local() -> PathBuf {
    PathBuf::from(env("LOCALAPPDATA"))
}

fn windir() -> PathBuf {
    PathBuf::from(if env("WINDIR").is_empty() { r"C:\Windows".to_string() } else { env("WINDIR") })
}

/// Chromium profile cache folders under a "User Data" folder.
fn chromium_caches(user_data: PathBuf) -> Vec<PathBuf> {
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir(&user_data) {
        for e in rd.filter_map(|e| e.ok()) {
            let n = e.file_name().to_string_lossy().to_string();
            if n == "Default" || n.starts_with("Profile ") {
                v.push(e.path().join(r"Cache\Cache_Data"));
                v.push(e.path().join("Code Cache"));
                v.push(e.path().join("GPUCache"));
            }
        }
    }
    v
}

fn firefox_caches() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir(local().join(r"Mozilla\Firefox\Profiles")) {
        for e in rd.filter_map(|e| e.ok()) {
            v.push(e.path().join("cache2"));
        }
    }
    v
}

fn target_paths(id: &str) -> Vec<PathBuf> {
    match id {
        "temp" => vec![PathBuf::from(env("TEMP")), windir().join("Temp")],
        "update" => vec![windir().join(r"SoftwareDistribution\Download")],
        "delivery" => vec![windir().join(r"ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache")],
        "dumps" => vec![
            windir().join("Minidump"),
            local().join("CrashDumps"),
            PathBuf::from(env("ProgramData")).join(r"Microsoft\Windows\WER\ReportArchive"),
            PathBuf::from(env("ProgramData")).join(r"Microsoft\Windows\WER\ReportQueue"),
        ],
        "thumbs" => vec![local().join(r"Microsoft\Windows\Explorer")],
        "chrome" => chromium_caches(local().join(r"Google\Chrome\User Data")),
        "edge" => chromium_caches(local().join(r"Microsoft\Edge\User Data")),
        "brave" => chromium_caches(local().join(r"BraveSoftware\Brave-Browser\User Data")),
        "firefox" => firefox_caches(),
        _ => vec![],
    }
}

const TARGETS: &[(&str, &str, &str)] = &[
    ("temp", "Temporary files", "Leftovers apps forgot to delete"),
    ("update", "Windows Update cache", "Already-installed update downloads"),
    ("delivery", "Delivery Optimization", "Update pieces shared with other PCs"),
    ("dumps", "Crash dumps & error reports", "Logs from old crashes"),
    ("thumbs", "Thumbnail cache", "Rebuilt automatically when needed"),
    ("chrome", "Chrome cache", "Skipped while Chrome is open"),
    ("edge", "Edge cache", "Skipped while Edge is open"),
    ("brave", "Brave cache", "Skipped while Brave is open"),
    ("firefox", "Firefox cache", "Skipped while Firefox is open"),
    ("recycle", "Recycle Bin", "Files you already deleted"),
];

fn browser_proc(id: &str) -> Option<&'static str> {
    match id {
        "chrome" => Some("chrome.exe"),
        "edge" => Some("msedge.exe"),
        "brave" => Some("brave.exe"),
        "firefox" => Some("firefox.exe"),
        _ => None,
    }
}

fn dir_size(p: &Path, thumbs_only: bool) -> u64 {
    let mut total = 0;
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if !thumbs_only {
                    stack.push(e.path());
                }
            } else if !thumbs_only || e.file_name().to_string_lossy().starts_with("thumbcache_") {
                total += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

fn clear_dir(p: &Path, thumbs_only: bool) {
    let Ok(rd) = std::fs::read_dir(p) else { return };
    for e in rd.filter_map(|e| e.ok()) {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            if !thumbs_only {
                let _ = std::fs::remove_dir_all(e.path());
            }
        } else if !thumbs_only || e.file_name().to_string_lossy().starts_with("thumbcache_") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn recycle_size() -> u64 {
    ps("$s=(New-Object -ComObject Shell.Application).NameSpace(10).Items() | Measure-Object -Property Size -Sum; [int64]$s.Sum")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

fn target_size(id: &str) -> u64 {
    if id == "recycle" {
        return recycle_size();
    }
    let thumbs = id == "thumbs";
    target_paths(id).iter().map(|p| dir_size(p, thumbs)).sum()
}

pub fn cleanup_scan() -> Vec<CleanTarget> {
    TARGETS
        .iter()
        .map(|(id, name, desc)| CleanTarget { id: id.to_string(), name: name.to_string(), desc: desc.to_string(), bytes: target_size(id) })
        .collect()
}

fn running(exe: &str) -> bool {
    let mut sys = crate::state::lock(&crate::state::core().sys);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let found = sys.processes().values().any(|p| p.name().to_string_lossy().eq_ignore_ascii_case(exe));
    found
}

/// Clean the given targets, returns bytes freed.
pub fn cleanup_run(ids: &[String], module: &str) -> u64 {
    let mut freed = 0u64;
    for id in ids {
        if let Some(b) = browser_proc(id) {
            if running(b) {
                continue;
            }
        }
        let before = target_size(id);
        if id == "recycle" {
            let _ = ps("Clear-RecycleBin -Force -ErrorAction SilentlyContinue");
        } else {
            if id == "update" {
                let _ = win::run("net.exe", &["stop", "wuauserv"]);
            }
            let thumbs = id == "thumbs";
            for p in target_paths(id) {
                clear_dir(&p, thumbs);
            }
            if id == "update" {
                let _ = win::run("net.exe", &["start", "wuauserv"]);
            }
        }
        let after = target_size(id);
        freed += before.saturating_sub(after);
    }
    if freed > 0 {
        changelog::record(module, "Junk cleaned", &crate::fmt_bytes(freed), Change::Files { bytes: freed });
    }
    with_cfg(|cf| {
        cf.stats.freed_bytes += freed;
        cf.stats.cleanups += 1;
        cf.last_cleanup = now();
    });
    freed
}

pub fn all_clean_ids() -> Vec<String> {
    TARGETS.iter().map(|t| t.0.to_string()).collect()
}

// ================================================================== debloat

#[derive(Serialize, Clone)]
pub struct BloatApp {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub installed: bool,
}

const BLOAT: &[(&str, &str, &str)] = &[
    ("Microsoft.BingNews", "Microsoft News", "News feed app"),
    ("Microsoft.BingWeather", "Weather", "MSN Weather app"),
    ("Microsoft.BingSearch", "Bing Search", "Bing search extras"),
    ("Microsoft.GetHelp", "Get Help", "Microsoft support app"),
    ("Microsoft.Getstarted", "Tips", "Windows tips app"),
    ("Microsoft.MicrosoftSolitaireCollection", "Solitaire Collection", "Ad-supported card games"),
    ("Microsoft.MicrosoftOfficeHub", "Microsoft 365 (Office hub)", "Office launcher/upsell"),
    ("Microsoft.People", "People", "Contacts app"),
    ("Microsoft.PowerAutomateDesktop", "Power Automate", "Automation tool"),
    ("Microsoft.Todos", "Microsoft To Do", "To-do list app"),
    ("Microsoft.WindowsFeedbackHub", "Feedback Hub", "Sends feedback to Microsoft"),
    ("Microsoft.WindowsMaps", "Maps", "Offline maps app"),
    ("Microsoft.ZuneVideo", "Movies & TV", "Old video app"),
    ("Microsoft.549981C3F5F10", "Cortana", "Old assistant"),
    ("Microsoft.MicrosoftJournal", "Journal", "Pen notes app"),
    ("Microsoft.Windows.DevHome", "Dev Home", "Developer dashboard"),
    ("Microsoft.YourPhone", "Phone Link", "Connects your phone"),
    ("MicrosoftCorporationII.MicrosoftFamily", "Family Safety", "Parental controls app"),
    ("Clipchamp.Clipchamp", "Clipchamp", "Video editor"),
    ("MicrosoftTeams", "Teams (personal)", "Chat app pinned by Windows"),
    ("MSTeams", "Teams (new)", "Chat app pinned by Windows"),
    ("king.com.*", "Candy Crush & King games", "Preinstalled games"),
    ("*Disney*", "Disney+", "Preinstalled streaming app"),
    ("*SpotifyAB*", "Spotify (preinstalled)", "Only if Windows preinstalled it"),
    ("*TikTok*", "TikTok", "Preinstalled app"),
    ("*Instagram*", "Instagram", "Preinstalled app"),
    ("*Facebook*", "Facebook", "Preinstalled app"),
    ("*AmazonPrimeVideo*", "Prime Video", "Preinstalled app"),
    ("Microsoft.XboxApp", "Xbox Console Companion", "Old Xbox app"),
];

fn wildcard_match(pat: &str, s: &str) -> bool {
    let p = pat.to_lowercase();
    let s = s.to_lowercase();
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == s;
    }
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match s[pos..].find(part) {
            Some(idx) => {
                if i == 0 && idx != 0 {
                    return false;
                }
                pos += idx + part.len();
            }
            None => return false,
        }
    }
    if !p.ends_with('*') {
        return s.ends_with(parts.last().unwrap_or(&""));
    }
    true
}

pub fn debloat_list() -> Vec<BloatApp> {
    let installed: Vec<String> = ps_json("ConvertTo-Json -InputObject @(Get-AppxPackage -AllUsers | Select-Object -ExpandProperty Name) -Compress")
        .into_iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    BLOAT
        .iter()
        .map(|(id, name, desc)| BloatApp {
            id: id.to_string(),
            name: name.to_string(),
            desc: desc.to_string(),
            installed: installed.iter().any(|n| wildcard_match(id, n)),
        })
        .collect()
}

pub fn debloat_remove(ids: &[String]) -> (u32, Vec<String>) {
    let mut n = 0;
    let mut errs = vec![];
    for id in ids {
        if !BLOAT.iter().any(|b| b.0 == id.as_str()) {
            continue;
        }
        let q = ps_quote(id);
        let script = format!(
            "Get-AppxPackage -AllUsers -Name {q} | Remove-AppxPackage -AllUsers -ErrorAction SilentlyContinue; \
             Get-AppxProvisionedPackage -Online | Where-Object {{ $_.DisplayName -like {q} }} | Remove-AppxProvisionedPackage -Online -ErrorAction SilentlyContinue | Out-Null"
        );
        match ps(&script) {
            Ok(_) => {
                n += 1;
                let name = BLOAT.iter().find(|b| b.0 == id.as_str()).map(|b| b.1).unwrap_or(id.as_str());
                changelog::record("Speed", &format!("Removed {name}"), "Preinstalled app", Change::AppxRemoved { name: id.clone() });
            }
            Err(e) => errs.push(format!("{id}: {e}")),
        }
    }
    (n, errs)
}

// ================================================================== services

#[derive(Serialize, Clone)]
pub struct ServiceItem {
    pub name: String,
    pub display: String,
    pub desc: String,
    pub group: String,
    pub start: u32,
    pub exists: bool,
}

const SERVICES: &[(&str, &str, &str, &str)] = &[
    ("DiagTrack", "Connected User Experiences and Telemetry", "Sends usage data to Microsoft", "telemetry"),
    ("dmwappushservice", "WAP Push Message Routing", "Telemetry message routing", "telemetry"),
    ("RetailDemo", "Retail Demo", "Only used on shop display PCs", "junk"),
    ("MapsBroker", "Downloaded Maps Manager", "Only needed for the Maps app", "junk"),
    ("Fax", "Fax", "Fax machines", "junk"),
    ("WMPNetworkSvc", "Windows Media Player Sharing", "Shares media libraries on your network", "junk"),
    ("RemoteRegistry", "Remote Registry", "Lets other PCs edit your registry (security risk)", "junk"),
    ("XblAuthManager", "Xbox Live Auth Manager", "Only if you don't use Xbox / Game Pass", "xbox"),
    ("XblGameSave", "Xbox Live Game Save", "Only if you don't use Xbox / Game Pass", "xbox"),
    ("XboxNetApiSvc", "Xbox Live Networking", "Only if you don't use Xbox / Game Pass", "xbox"),
];

fn svc_key(name: &str) -> String {
    format!(r"SYSTEM\CurrentControlSet\Services\{name}")
}

pub fn services_list() -> Vec<ServiceItem> {
    SERVICES
        .iter()
        .map(|(n, d, desc, g)| {
            let start = win::reg_dword(Hive::HKLM, &svc_key(n), "Start");
            ServiceItem {
                name: n.to_string(),
                display: d.to_string(),
                desc: desc.to_string(),
                group: g.to_string(),
                start: start.unwrap_or(0),
                exists: start.is_some(),
            }
        })
        .collect()
}

pub fn set_service_start(name: &str, start: u32) -> Result<(), String> {
    win::reg_set(Hive::HKLM, &svc_key(name), "Start", &RegVal::Dword(start))?;
    if start == 4 {
        let _ = win::run("sc.exe", &["stop", name]);
    }
    Ok(())
}

pub fn service_set(name: &str, disabled: bool, module: &str) -> Result<(), String> {
    if !SERVICES.iter().any(|s| s.0 == name) {
        return Err("Nightmoth only changes services on its safe list".into());
    }
    let before = win::reg_dword(Hive::HKLM, &svc_key(name), "Start").ok_or("Service not installed")?;
    let after = if disabled { 4 } else if before == 4 { 3 } else { before };
    if before == after {
        return Ok(());
    }
    set_service_start(name, after)?;
    let verb = if disabled { "Disabled" } else { "Re-enabled" };
    changelog::record(module, &format!("{verb} service {name}"), "", Change::ServiceStart { name: name.into(), before, after });
    Ok(())
}

// ================================================================== privacy

#[derive(Serialize, Clone)]
pub struct PrivacyItem {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub on: bool,
}

const CDM: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";

/// (hive, path, name, value when protected, value when not (None = delete))
type Tweak = (Hive, &'static str, &'static str, u32, Option<u32>);

fn privacy_tweaks(id: &str) -> Vec<Tweak> {
    match id {
        "ads_id" => vec![(Hive::HKCU, r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo", "Enabled", 0, Some(1))],
        "start_ads" => vec![
            (Hive::HKCU, CDM, "SubscribedContent-338388Enabled", 0, Some(1)),
            (Hive::HKCU, CDM, "SystemPaneSuggestionsEnabled", 0, Some(1)),
            (Hive::HKCU, CDM, "SilentInstalledAppsEnabled", 0, Some(1)),
            (Hive::HKCU, CDM, "PreInstalledAppsEnabled", 0, Some(1)),
            (Hive::HKCU, CDM, "OemPreInstalledAppsEnabled", 0, Some(1)),
            (Hive::HKCU, r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced", "Start_IrisRecommendations", 0, Some(1)),
        ],
        "lock_tips" => vec![
            (Hive::HKCU, CDM, "RotatingLockScreenOverlayEnabled", 0, Some(1)),
            (Hive::HKCU, CDM, "SubscribedContent-338387Enabled", 0, Some(1)),
        ],
        "tips" => vec![
            (Hive::HKCU, CDM, "SubscribedContent-338389Enabled", 0, Some(1)),
            (Hive::HKCU, CDM, "SubscribedContent-310093Enabled", 0, Some(1)),
            (Hive::HKCU, CDM, "SubscribedContent-353694Enabled", 0, Some(1)),
            (Hive::HKCU, CDM, "SubscribedContent-353696Enabled", 0, Some(1)),
            (Hive::HKCU, CDM, "SoftLandingEnabled", 0, Some(1)),
        ],
        "explorer_ads" => vec![(Hive::HKCU, r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced", "ShowSyncProviderNotifications", 0, Some(1))],
        "bing" => vec![
            (Hive::HKCU, r"Software\Policies\Microsoft\Windows\Explorer", "DisableSearchBoxSuggestions", 1, None),
            (Hive::HKCU, r"Software\Microsoft\Windows\CurrentVersion\Search", "BingSearchEnabled", 0, Some(1)),
        ],
        "telemetry" => vec![
            (Hive::HKLM, r"SOFTWARE\Policies\Microsoft\Windows\DataCollection", "AllowTelemetry", 0, None),
            (Hive::HKCU, r"Software\Microsoft\Windows\CurrentVersion\Privacy", "TailoredExperiencesWithDiagnosticDataEnabled", 0, Some(1)),
        ],
        "activity" => vec![
            (Hive::HKLM, r"SOFTWARE\Policies\Microsoft\Windows\System", "PublishUserActivities", 0, None),
            (Hive::HKLM, r"SOFTWARE\Policies\Microsoft\Windows\System", "UploadUserActivities", 0, None),
        ],
        _ => vec![],
    }
}

const PRIVACY: &[(&str, &str, &str)] = &[
    ("ads_id", "Advertising ID", "Stop apps tracking you for personalised ads"),
    ("start_ads", "Start menu ads", "No suggested or auto-installed apps in Start"),
    ("lock_tips", "Lock screen ads & tips", "Clean lock screen with just your picture"),
    ("tips", "Tips & suggestions", "No 'get even more out of Windows' pop-ups"),
    ("explorer_ads", "File Explorer ads", "No OneDrive/Microsoft 365 promos in Explorer"),
    ("bing", "Bing in Start search", "Start search only searches your PC"),
    ("telemetry", "Telemetry", "Send Microsoft the minimum diagnostic data"),
    ("activity", "Activity history", "Don't record and upload what you do"),
];

pub fn privacy_list() -> Vec<PrivacyItem> {
    PRIVACY
        .iter()
        .map(|(id, name, desc)| {
            let on = privacy_tweaks(id).iter().all(|(h, p, n, v, _)| win::reg_dword(*h, p, n) == Some(*v));
            PrivacyItem { id: id.to_string(), name: name.to_string(), desc: desc.to_string(), on }
        })
        .collect()
}

pub fn privacy_set(id: &str, on: bool, module: &str) -> Result<(), String> {
    let mut ch = vec![];
    for (h, p, n, v_on, v_off) in privacy_tweaks(id) {
        let c = if on {
            changelog::reg(h, p, n, RegVal::Dword(v_on))?
        } else {
            match v_off {
                Some(v) => changelog::reg(h, p, n, RegVal::Dword(v))?,
                None => changelog::reg_del(h, p, n)?,
            }
        };
        ch.extend(c);
    }
    let name = PRIVACY.iter().find(|x| x.0 == id).map(|x| x.1).unwrap_or(id);
    let verb = if on { "Blocked" } else { "Allowed" };
    changelog::record_many(module, &format!("{verb}: {name}"), "", ch);
    win::broadcast("ImmersiveColorSet");
    Ok(())
}

// ================================================================== power

#[derive(Serialize, Clone)]
pub struct Plan {
    pub guid: String,
    pub name: String,
    pub active: bool,
}

const ULTIMATE_TEMPLATE: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";

fn find_guid(s: &str) -> Option<String> {
    s.split(|c: char| !(c.is_ascii_hexdigit() || c == '-'))
        .find(|t| t.len() == 36 && t.matches('-').count() == 4)
        .map(|t| t.to_lowercase())
}

pub fn plans() -> Vec<Plan> {
    let out = win::run("powercfg.exe", &["/list"]).unwrap_or_default();
    out.lines()
        .filter_map(|l| {
            let guid = find_guid(l)?;
            let name = l.split('(').nth(1).and_then(|r| r.split(')').next()).unwrap_or("").trim().to_string();
            Some(Plan { guid, name, active: l.trim_end().ends_with('*') })
        })
        .collect()
}

pub fn active_plan() -> String {
    win::run("powercfg.exe", &["/getactivescheme"]).ok().and_then(|s| find_guid(&s)).unwrap_or_default()
}

pub fn set_active_plan(guid: &str) -> Result<(), String> {
    win::run("powercfg.exe", &["/setactive", guid]).map(|_| ())
}

/// Ultimate Performance plan GUID (created from Windows' hidden template if needed).
pub fn ultimate_guid() -> Result<String, String> {
    let saved = cfg().ultimate_guid;
    let list = plans();
    if !saved.is_empty() && list.iter().any(|p| p.guid == saved) {
        return Ok(saved);
    }
    if let Some(p) = list.iter().find(|p| p.guid == ULTIMATE_TEMPLATE) {
        return Ok(p.guid.clone());
    }
    let out = win::run("powercfg.exe", &["-duplicatescheme", ULTIMATE_TEMPLATE])?;
    let guid = find_guid(&out).ok_or("Windows didn't create the Ultimate Performance plan")?;
    let _ = win::run("powercfg.exe", &["-changename", &guid, "Nightmoth Ultimate", "Maximum performance (Nightmoth)"]);
    with_cfg(|c| c.ultimate_guid = guid.clone());
    Ok(guid)
}

pub fn set_plan_logged(guid: &str, module: &str) -> Result<(), String> {
    let before = active_plan();
    if before == guid {
        return Ok(());
    }
    set_active_plan(guid)?;
    changelog::record(module, "Power plan changed", "", Change::PowerPlan { before, after: guid.into() });
    Ok(())
}

#[derive(Serialize, Clone)]
pub struct PowerState {
    pub plans: Vec<Plan>,
    pub ultimate: String,
    pub hags: bool,
    pub game_mode: bool,
}

const GFX: &str = r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers";
const GAMEBAR: &str = r"Software\Microsoft\GameBar";

pub fn power_state() -> PowerState {
    PowerState {
        plans: plans(),
        ultimate: cfg().ultimate_guid,
        hags: win::reg_dword(Hive::HKLM, GFX, "HwSchMode") == Some(2),
        game_mode: win::reg_dword(Hive::HKCU, GAMEBAR, "AutoGameModeEnabled").unwrap_or(1) == 1,
    }
}

pub fn hags_set(on: bool, module: &str) -> Result<(), String> {
    let c = changelog::reg(Hive::HKLM, GFX, "HwSchMode", RegVal::Dword(if on { 2 } else { 1 }))?;
    changelog::record_many(module, if on { "GPU scheduling on (restart needed)" } else { "GPU scheduling off" }, "", c.into_iter().collect());
    Ok(())
}

pub fn game_mode_set(on: bool, module: &str) -> Result<(), String> {
    let mut ch = vec![];
    ch.extend(changelog::reg(Hive::HKCU, GAMEBAR, "AutoGameModeEnabled", RegVal::Dword(on as u32))?);
    ch.extend(changelog::reg(Hive::HKCU, GAMEBAR, "AllowAutoGameMode", RegVal::Dword(on as u32))?);
    changelog::record_many(module, if on { "Windows Game Mode on" } else { "Windows Game Mode off" }, "", ch);
    Ok(())
}

// ================================================================== boot time

#[derive(Serialize, Clone)]
pub struct Boot {
    pub t: i64,
    pub ms: i64,
}

pub fn boot_times() -> Vec<Boot> {
    let script = r#"$e = Get-WinEvent -FilterHashtable @{LogName='Microsoft-Windows-Diagnostics-Performance/Operational'; Id=100} -MaxEvents 12 -ErrorAction SilentlyContinue; $r = foreach($x in $e){ $xml=[xml]$x.ToXml(); $bt = ($xml.Event.EventData.Data | Where-Object { $_.Name -eq 'BootTime' }).'#text'; [pscustomobject]@{ t=[int64](($x.TimeCreated.ToUniversalTime() - [datetime]'1970-01-01').TotalSeconds); ms=[int64]$bt } }; ConvertTo-Json -InputObject @($r) -Compress"#;
    let mut v: Vec<Boot> = ps_json(script)
        .into_iter()
        .filter_map(|x| Some(Boot { t: x["t"].as_i64()?, ms: x["ms"].as_i64()? }))
        .collect();
    v.sort_by_key(|b| b.t);
    v
}

// ================================================================== safety

pub fn restore_point(force: bool) -> Result<(), String> {
    let c = cfg();
    if !force && now() - c.last_restore_point < 6 * 3600 {
        return Ok(());
    }
    let script = r#"Enable-ComputerRestore -Drive "$env:SystemDrive\" -ErrorAction SilentlyContinue; New-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore' -Name SystemRestorePointCreationFrequency -Value 0 -PropertyType DWord -Force | Out-Null; Checkpoint-Computer -Description 'Nightmoth' -RestorePointType MODIFY_SETTINGS"#;
    ps(script)?;
    with_cfg(|cf| cf.last_restore_point = now());
    Ok(())
}

// ================================================================== one click

#[derive(Serialize, Clone)]
pub struct OptimizeReport {
    pub freed: u64,
    pub privacy: u32,
    pub services: u32,
    pub restore_point: bool,
}

pub fn optimize_now() -> OptimizeReport {
    let rp = restore_point(false).is_ok();
    let freed = cleanup_run(&all_clean_ids().into_iter().filter(|i| i != "recycle").collect::<Vec<_>>(), "Optimize");
    let mut privacy = 0;
    for p in privacy_list() {
        if !p.on && privacy_set(&p.id, true, "Optimize").is_ok() {
            privacy += 1;
        }
    }
    let mut services = 0;
    for s in services_list() {
        if s.exists && s.group != "xbox" && s.start != 4 && service_set(&s.name, true, "Optimize").is_ok() {
            services += 1;
        }
    }
    let _ = game_mode_set(true, "Optimize");
    toast::show(
        "optimize",
        "PC optimized",
        &format!("Freed {} · {} privacy fixes · {} services off", crate::fmt_bytes(freed), privacy, services),
    );
    OptimizeReport { freed, privacy, services, restore_point: rp }
}
