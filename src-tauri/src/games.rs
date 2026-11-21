//! Detect games, boost them, restore everything when they close.

use serde::Serialize;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;
use sysinfo::ProcessesToUpdate;

use crate::config::{BoostSession, GameCfg};
use crate::state::{cfg, core, lock, with_cfg};
use crate::toast;
use crate::win::{self, now, Hive, Priority, RegVal};

const KNOWN: &[(&str, &str)] = &[
    ("robloxplayerbeta.exe", "Roblox"),
    ("minecraft.windows.exe", "Minecraft"),
    ("fortniteclient-win64-shipping.exe", "Fortnite"),
    ("valorant-win64-shipping.exe", "VALORANT"),
    ("league of legends.exe", "League of Legends"),
    ("cs2.exe", "Counter-Strike 2"),
    ("r5apex.exe", "Apex Legends"),
    ("r5apex_dx12.exe", "Apex Legends"),
    ("gta5.exe", "GTA V"),
    ("gta5_enhanced.exe", "GTA V"),
    ("rdr2.exe", "Red Dead Redemption 2"),
    ("eldenring.exe", "Elden Ring"),
    ("cyberpunk2077.exe", "Cyberpunk 2077"),
    ("overwatch.exe", "Overwatch 2"),
    ("rocketleague.exe", "Rocket League"),
    ("projectzomboid64.exe", "Project Zomboid"),
    ("destiny2.exe", "Destiny 2"),
    ("rainbowsix.exe", "Rainbow Six Siege"),
    ("rainbowsix_vulkan.exe", "Rainbow Six Siege"),
    ("cod.exe", "Call of Duty"),
    ("cod24-cod.exe", "Call of Duty"),
    ("bf2042.exe", "Battlefield 2042"),
    ("marvel-win64-shipping.exe", "Marvel Rivals"),
    ("deltaforceclient-win64-shipping.exe", "Delta Force"),
    ("thefinals.exe", "THE FINALS"),
    ("pubg-win64-shipping.exe", "PUBG"),
    ("tslgame.exe", "PUBG"),
    ("dota2.exe", "Dota 2"),
    ("terraria.exe", "Terraria"),
    ("factorio.exe", "Factorio"),
    ("eurotrucks2.exe", "Euro Truck Simulator 2"),
    ("forzahorizon5.exe", "Forza Horizon 5"),
];

/// Executables that live in game folders but aren't games.
const NOT_GAMES: &[&str] = &[
    "crash", "launcher", "helper", "setup", "unins", "redist", "easyanticheat", "beservice", "battleye",
    "vcredist", "dxsetup", "dotnet", "report", "updater", "cefprocess", "webhelper", "steamservice",
    "epicgameslauncher", "unrealcefsubprocess", "riotclient", "overlay", "anticheat", "install",
];

#[derive(Serialize, Clone, Debug)]
pub struct GameInfo {
    pub exe: String,
    pub name: String,
    pub path: String,
    pub boost: bool,
    pub plays: u32,
    pub last_played: i64,
    pub minutes: u64,
    pub running: bool,
}

fn steam_libraries() -> Vec<String> {
    let mut libs = vec![];
    if let Some(root) = win::reg_str(Hive::HKCU, r"Software\Valve\Steam", "SteamPath") {
        let root = root.replace('/', "\\");
        libs.push(format!(r"{root}\steamapps\common").to_lowercase());
        if let Ok(vdf) = std::fs::read_to_string(format!(r"{root}\steamapps\libraryfolders.vdf")) {
            for line in vdf.lines() {
                let t = line.trim();
                if t.starts_with("\"path\"") {
                    if let Some(p) = t.split('"').nth(3) {
                        libs.push(format!(r"{}\steamapps\common", p.replace("\\\\", "\\")).to_lowercase());
                    }
                }
            }
        }
    }
    libs
}

/// Decide if a running exe is a game. Returns its display name.
fn classify(exe: &str, path: &str, libs: &[String]) -> Option<String> {
    let exe_l = exe.to_lowercase();
    if let Some((_, name)) = KNOWN.iter().find(|(e, _)| *e == exe_l) {
        return Some(name.to_string());
    }
    if NOT_GAMES.iter().any(|n| exe_l.contains(n)) {
        return None;
    }
    let p = path.to_lowercase();
    let game_dir = libs.iter().find(|l| !l.is_empty() && p.starts_with(l.as_str())).map(|l| l.len())
        .or_else(|| ["\\epic games\\", "\\riot games\\", "\\xboxgames\\", "\\gog games\\", "\\ea games\\", "\\ubisoft game launcher\\games\\"]
            .iter()
            .find_map(|m| p.find(m).map(|i| i + m.len() - 1)));
    let cut = game_dir?;
    // folder right after the library root is the game name
    let rest = path.get(cut + 1..).unwrap_or("");
    let folder = rest.split('\\').next().unwrap_or("");
    if folder.is_empty() {
        return None;
    }
    Some(folder.to_string())
}

pub fn list() -> Vec<GameInfo> {
    let c = cfg();
    let running: Vec<String> = c.pending_boost.iter().map(|b| b.exe.clone()).collect();
    let mut v: Vec<GameInfo> = c
        .games
        .iter()
        .map(|(exe, g)| GameInfo {
            exe: exe.clone(),
            name: g.name.clone(),
            path: g.exe_path.clone(),
            boost: g.boost,
            plays: g.plays,
            last_played: g.last_played,
            minutes: g.minutes,
            running: running.contains(exe),
        })
        .collect();
    v.sort_by(|a, b| b.last_played.cmp(&a.last_played));
    v
}

pub fn set_boost(exe: &str, on: bool) {
    with_cfg(|c| {
        if let Some(g) = c.games.get_mut(exe) {
            g.boost = on;
        }
    });
}

#[derive(Serialize, Clone)]
pub struct Proc {
    pub name: String,
    pub pid: u32,
    pub mem_mb: u64,
}

/// User-facing running apps (for choosing what to close during games).
pub fn processes() -> Vec<Proc> {
    let mut sys = lock(&core().sys);
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let mut seen = std::collections::HashMap::<String, Proc>::new();
    for (pid, p) in sys.processes() {
        let name = p.name().to_string_lossy().to_string();
        let exe = p.exe().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if exe.is_empty() || exe.contains("\\windows\\") {
            continue;
        }
        let e = seen.entry(name.to_lowercase()).or_insert(Proc { name: name.clone(), pid: pid.as_u32(), mem_mb: 0 });
        e.mem_mb += p.memory() / 1024 / 1024;
    }
    let mut v: Vec<Proc> = seen.into_values().collect();
    v.sort_by(|a, b| b.mem_mb.cmp(&a.mem_mb));
    v
}

const NOTIF: &str = r"Software\Microsoft\Windows\CurrentVersion\PushNotifications";
const DO_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization";
/// While gaming, Windows Update may only use a sliver of the connection.
const DO_LIMITS: &[(&str, u32)] = &[("DOPercentageMaxForegroundBandwidth", 10), ("DOPercentageMaxBackgroundBandwidth", 5)];

/// Throttle update downloads and stop (not disable) the update services for the game.
fn pause_updates() -> Vec<(String, Option<u32>)> {
    let mut prev = vec![];
    for (name, v) in DO_LIMITS {
        prev.push((name.to_string(), win::reg_dword(Hive::HKLM, DO_POLICY, name)));
        let _ = win::reg_set(Hive::HKLM, DO_POLICY, name, &RegVal::Dword(*v));
    }
    for svc in ["UsoSvc", "wuauserv"] {
        let _ = win::run("sc.exe", &["stop", svc]);
    }
    prev
}

fn resume_updates(prev: &[(String, Option<u32>)]) {
    for (name, v) in prev {
        match v {
            Some(v) => {
                let _ = win::reg_set(Hive::HKLM, DO_POLICY, name, &RegVal::Dword(*v));
            }
            None => {
                let _ = win::reg_delete(Hive::HKLM, DO_POLICY, name);
            }
        }
    }
}

fn start_boost(name: &str, exe: &str, path: &str, pid: u32) {
    core().boosting.store(true, Ordering::Relaxed);
    let c = cfg();
    let prev_plan = crate::speed::active_plan();
    if let Ok(g) = crate::speed::ultimate_guid() {
        let _ = crate::speed::set_active_plan(&g);
    }
    win::set_priority(pid, Priority::High);
    let prev_toasts = win::reg_dword(Hive::HKCU, NOTIF, "ToastEnabled");
    let _ = win::reg_set(Hive::HKCU, NOTIF, "ToastEnabled", &RegVal::Dword(0));

    // close the apps the user picked, remembering how to start them again
    let mut closed = vec![];
    {
        let mut sys = lock(&core().sys);
        sys.refresh_processes(ProcessesToUpdate::All, true);
        for p in sys.processes().values() {
            let pname = p.name().to_string_lossy().to_lowercase();
            if c.boost_close.iter().any(|b| b.to_lowercase() == pname) {
                if let Some(e) = p.exe() {
                    let e = e.to_string_lossy().to_string();
                    if !closed.contains(&e) {
                        closed.push(e);
                    }
                }
                win::kill_process(p.pid().as_u32());
            }
        }
    }

    let prev_do = if c.wu_control { pause_updates() } else { vec![] };
    let session = BoostSession {
        game: name.into(),
        exe: exe.into(),
        pid,
        started: now(),
        prev_plan,
        prev_toasts,
        closed: closed.clone(),
        prev_do,
    };
    with_cfg(|cf| {
        let g = cf.games.entry(exe.to_string()).or_insert_with(|| GameCfg { name: name.into(), exe_path: path.into(), boost: true, ..Default::default() });
        g.plays += 1;
        g.last_played = now();
        g.exe_path = path.into();
        cf.stats.games_boosted += 1;
        cf.pending_boost = Some(session);
    });
    let extra = if closed.is_empty() { String::new() } else { format!(" · {} apps closed", closed.len()) };
    toast::show("game", &format!("{name} boosted"), &format!("Max power · high priority · updates paused{extra}"));
}

/// Undo a boost (also used at startup if the app crashed mid-boost).
pub fn end_boost(announce: bool) {
    let Some(s) = cfg().pending_boost else {
        core().boosting.store(false, Ordering::Relaxed);
        return;
    };
    if !s.prev_plan.is_empty() {
        let _ = crate::speed::set_active_plan(&s.prev_plan);
    }
    resume_updates(&s.prev_do);
    match s.prev_toasts {
        Some(v) => {
            let _ = win::reg_set(Hive::HKCU, NOTIF, "ToastEnabled", &RegVal::Dword(v));
        }
        None => {
            let _ = win::reg_delete(Hive::HKCU, NOTIF, "ToastEnabled");
        }
    }
    // reopen them as the normal user (never with the engine's admin rights)
    for exe in &s.closed {
        if Path::new(exe).exists() {
            let _ = crate::userjob::run("launch", exe, None);
        }
    }
    let mins = ((now() - s.started).max(0) / 60) as u64;
    with_cfg(|cf| {
        if let Some(g) = cf.games.get_mut(&s.exe) {
            g.minutes += mins;
        }
        cf.pending_boost = None;
    });
    core().boosting.store(false, Ordering::Relaxed);
    if announce {
        toast::show("game", "Back to normal", &format!("{} closed after {} min · everything restored", s.game, mins));
    }
}

/// Apps that go fullscreen but aren't games.
const FULLSCREEN_NOT_GAMES: &[&str] = &[
    "explorer", "chrome", "msedge", "firefox", "brave", "opera", "vivaldi", "arc", "vlc", "mpv", "mpc-hc", "potplayer", "wmplayer",
    "video.ui", "microsoft.media.player", "netflix", "spotify", "obs64", "powerpnt", "nightmoth", "applicationframehost", "searchhost",
    "startmenuexperiencehost", "lockapp", "shellexperiencehost", "textinputhost", "discord", "code", "mstsc", "vmware", "virtualbox",
    "parsec", "moonlight", "photos", "acrobat", "winword", "excel", "teams", "zoom", "screenclippinghost", "snippingtool", "taskmgr",
];

/// The process whose window covers the whole screen right now (fullscreen or borderless).
fn fullscreen_pid() -> Option<u32> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId};
    unsafe {
        let h = GetForegroundWindow();
        if h.is_null() {
            return None;
        }
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(h, &mut r) == 0 {
            return None;
        }
        let mon = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return None;
        }
        let m = mi.rcMonitor;
        if r.left > m.left || r.top > m.top || r.right < m.right || r.bottom < m.bottom {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        (pid != 0).then_some(pid)
    }
}

fn nice_name(exe: &str) -> String {
    let stem = exe.trim_end_matches(".exe").trim_end_matches(".EXE");
    let stem = stem.trim_end_matches("-Win64-Shipping").trim_end_matches("-win64-shipping").trim_end_matches("_dx12").trim_end_matches("64");
    let mut out = String::new();
    for (i, w) in stem.split(['_', '-']).filter(|w| !w.is_empty()).enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut c = w.chars();
        if let Some(f) = c.next() {
            out.extend(f.to_uppercase());
            out.push_str(c.as_str());
        }
    }
    if out.is_empty() {
        exe.to_string()
    } else {
        out
    }
}

/// Watches for games every few seconds.
pub fn watcher() {
    // finish a boost left over from a crash
    if cfg().pending_boost.is_some() {
        end_boost(false);
    }
    let mut libs = steam_libraries();
    let mut tick = 0u32;
    let mut fs_seen: Option<(u32, std::time::Instant)> = None;
    loop {
        std::thread::sleep(Duration::from_secs(3));
        tick += 1;
        if tick % 200 == 0 {
            libs = steam_libraries();
        }
        let c = cfg();
        if let Some(s) = c.pending_boost.clone() {
            let alive = {
                let mut sys = lock(&core().sys);
                sys.refresh_processes(ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(s.pid)]), true);
                sys.process(sysinfo::Pid::from_u32(s.pid)).is_some()
            };
            if !alive {
                end_boost(true);
            }
            continue;
        }
        if !c.autopilot || !c.game_booster {
            continue;
        }
        let found = {
            let mut sys = lock(&core().sys);
            sys.refresh_processes(ProcessesToUpdate::All, true);
            sys.processes().values().find_map(|p| {
                let exe = p.name().to_string_lossy().to_string();
                let path = p.exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
                let key = exe.to_lowercase();
                if let Some(g) = c.games.get(&key) {
                    if !g.boost {
                        return None;
                    }
                }
                classify(&exe, &path, &libs).map(|name| (name, key, path, p.pid().as_u32()))
            })
        };
        if let Some((name, key, path, pid)) = found {
            start_boost(&name, &key, &path, pid);
            fs_seen = None;
            continue;
        }
        // not on any list: anything that stays fullscreen for 10+ seconds is treated as a game
        match fullscreen_pid() {
            Some(pid) => {
                let info = {
                    let mut sys = lock(&core().sys);
                    sys.refresh_processes(ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]), true);
                    sys.process(sysinfo::Pid::from_u32(pid)).map(|p| {
                        (p.name().to_string_lossy().to_string(), p.exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_default())
                    })
                };
                let Some((exe, path)) = info else { continue };
                let key = exe.to_lowercase();
                let pl = path.to_lowercase();
                if FULLSCREEN_NOT_GAMES.iter().any(|n| key.contains(n)) || NOT_GAMES.iter().any(|n| key.contains(n)) || pl.contains("\\windows\\") {
                    fs_seen = None;
                    continue;
                }
                if c.games.get(&key).map(|g| !g.boost).unwrap_or(false) {
                    continue;
                }
                match fs_seen {
                    Some((p, since)) if p == pid => {
                        if since.elapsed() >= Duration::from_secs(10) {
                            start_boost(&nice_name(&exe), &key, &path, pid);
                            fs_seen = None;
                        }
                    }
                    _ => fs_seen = Some((pid, std::time::Instant::now())),
                }
            }
            None => fs_seen = None,
        }
    }
}
