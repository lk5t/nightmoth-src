//! Roblox best settings (app side, your own files): graphics quality picked for your
//! hardware and the frame cap raised to your monitor's refresh rate. The original
//! files are backed up first and can be put back. Only the settings Roblox itself
//! allows are touched; nothing is injected into the game.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

use crate::win::{self, user_dir};

#[derive(Serialize, Deserialize, Default)]
struct State {
    /// version folders we've written the FPS setting into
    versions: Vec<String>,
    quality_set: bool,
    tier: String,
    fps: u32,
}

fn state_path() -> PathBuf {
    user_dir().join("roblox.json")
}

fn load() -> State {
    std::fs::read_to_string(state_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(s: &State) {
    let _ = std::fs::write(state_path(), serde_json::to_string(s).unwrap_or_default());
}

fn roblox_dir() -> PathBuf {
    PathBuf::from(win::env("LOCALAPPDATA")).join("Roblox")
}

fn backup_dir() -> PathBuf {
    let d = user_dir().join("backups").join("roblox");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Monitor refresh rate of the main screen.
pub fn refresh_rate() -> u32 {
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS};
    let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
    dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
    let ok = unsafe { EnumDisplaySettingsW(std::ptr::null(), ENUM_CURRENT_SETTINGS, &mut dm) };
    if ok == 0 || dm.dmDisplayFrequency < 30 {
        60
    } else {
        dm.dmDisplayFrequency
    }
}

/// "high", "mid" or "low" from the graphics card name and RAM.
pub fn hardware_tier() -> String {
    let gpus = win::ps_json("ConvertTo-Json @((Get-CimInstance Win32_VideoController).Name) -Compress");
    let names: Vec<String> = gpus.iter().filter_map(|g| g.as_str().map(|s| s.to_lowercase())).collect();
    let ram = sysinfo::System::new_with_specifics(sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything())).total_memory();
    let any = |keys: &[&str]| names.iter().any(|n| keys.iter().any(|k| n.contains(k)));
    let tier = if any(&["rtx 50", "rtx 40", "rtx 30", "rtx 20", "rx 9", "rx 7", "rx 6", "arc b", "arc a7"]) {
        "high"
    } else if any(&["gtx 16", "gtx 10", "rx 5", "rx 580", "rx 590", "arc a5", "arc a3", "rtx a"]) {
        "mid"
    } else {
        "low"
    };
    if tier == "high" && ram < 12_000_000_000 {
        "mid".into()
    } else {
        tier.into()
    }
}

fn quality_for(tier: &str) -> u32 {
    match tier {
        "high" => 9,
        "mid" => 6,
        _ => 3,
    }
}

fn running() -> bool {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.processes().values().any(|p| p.name().to_string_lossy().eq_ignore_ascii_case("RobloxPlayerBeta.exe"))
}

pub fn installed() -> bool {
    roblox_dir().join("Versions").is_dir()
}

/// Player version folders (the ones with RobloxPlayerBeta.exe).
fn player_versions() -> Vec<PathBuf> {
    let mut dirs = vec![];
    for base in [roblox_dir().join("Versions"), PathBuf::from(win::env("ProgramFiles(x86)")).join(r"Roblox\Versions")] {
        if let Ok(rd) = std::fs::read_dir(base) {
            for e in rd.filter_map(|e| e.ok()) {
                if e.path().join("RobloxPlayerBeta.exe").exists() {
                    dirs.push(e.path());
                }
            }
        }
    }
    dirs
}

fn set_fps(dir: &PathBuf, fps: u32) -> Result<(), String> {
    let cs = dir.join("ClientSettings");
    std::fs::create_dir_all(&cs).map_err(|e| e.to_string())?;
    let f = cs.join("ClientAppSettings.json");
    let mut v: Value = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}));
    if !v.is_object() {
        v = json!({});
    }
    v["DFIntTaskSchedulerTargetFps"] = json!(fps);
    std::fs::write(&f, serde_json::to_string_pretty(&v).unwrap_or_default()).map_err(|e| e.to_string())
}

fn set_quality(level: u32) -> Result<(), String> {
    let f = roblox_dir().join("GlobalBasicSettings_13.xml");
    let xml = std::fs::read_to_string(&f).map_err(|_| "Open Roblox once first".to_string())?;
    let bak = backup_dir().join("GlobalBasicSettings_13.xml");
    if !bak.exists() {
        let _ = std::fs::write(&bak, &xml);
    }
    let mut out = xml.clone();
    let mut set = |tag: &str, name: &str, val: &str| {
        let open = format!("<{tag} name=\"{name}\">");
        if let Some(i) = out.find(&open) {
            let start = i + open.len();
            if let Some(j) = out[start..].find(&format!("</{tag}>")) {
                out.replace_range(start..start + j, val);
            }
        }
    };
    set("token", "SavedQualityLevel", &level.to_string());
    set("int", "GraphicsQualityLevel", &level.to_string());
    std::fs::write(&f, out).map_err(|e| e.to_string())
}

/// Apply now (Roblox must be closed). Returns what was set.
pub fn apply(force_quality: bool) -> Result<Value, String> {
    if !installed() {
        return Err("Roblox isn't installed".into());
    }
    if running() {
        return Err("Close Roblox first — it saves its own settings when it exits".into());
    }
    let mut st = load();
    let tier = hardware_tier();
    let fps = refresh_rate().max(60);
    let mut changed = false;
    for v in player_versions() {
        let name = v.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !st.versions.contains(&name) || st.fps != fps {
            set_fps(&v, fps)?;
            st.versions.push(name);
            changed = true;
        }
    }
    if !st.quality_set || force_quality {
        set_quality(quality_for(&tier))?;
        st.quality_set = true;
        changed = true;
    }
    st.tier = tier.clone();
    st.fps = fps;
    st.versions.sort();
    st.versions.dedup();
    save(&st);
    Ok(json!({ "tier": tier, "fps": fps, "quality": quality_for(&tier), "changed": changed }))
}

/// Put the original settings back.
pub fn restore() -> Result<(), String> {
    let bak = backup_dir().join("GlobalBasicSettings_13.xml");
    if bak.exists() {
        std::fs::copy(&bak, roblox_dir().join("GlobalBasicSettings_13.xml")).map_err(|e| e.to_string())?;
    }
    for v in player_versions() {
        let f = v.join(r"ClientSettings\ClientAppSettings.json");
        if let Ok(s) = std::fs::read_to_string(&f) {
            if let Ok(mut j) = serde_json::from_str::<Value>(&s) {
                if let Some(o) = j.as_object_mut() {
                    o.remove("DFIntTaskSchedulerTargetFps");
                    let _ = if o.is_empty() { std::fs::remove_file(&f) } else { std::fs::write(&f, serde_json::to_string_pretty(&j).unwrap_or_default()) };
                }
            }
        }
    }
    let _ = std::fs::remove_file(state_path());
    Ok(())
}

pub fn state() -> Value {
    let st = load();
    json!({ "installed": installed(), "tuned": st.quality_set, "tier": st.tier, "fps": st.fps, "refresh": refresh_rate() })
}

/// Roblox updates itself often (new version folder): re-apply after every update.
pub fn start_auto() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(10 * 60));
        let Ok(c) = crate::ipc::call_quiet("get_config", json!({})) else { continue };
        if !c["autopilot"].as_bool().unwrap_or(true) || !c["roblox_tune"].as_bool().unwrap_or(true) || !installed() || running() {
            continue;
        }
        if let Ok(r) = apply(false) {
            if r["changed"].as_bool().unwrap_or(false) {
                crate::ui_toast::local("Roblox tuned", &format!("{} FPS cap · graphics {} for your PC", r["fps"], r["quality"]));
            }
        }
    });
}
