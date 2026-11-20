//! Thin wrappers around Windows: registry, PowerShell, shell messages, wallpaper,
//! taskbar, desktop icons, process priority.

use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::ptr::{null, null_mut};
use winreg::enums::*;
use winreg::{RegKey, RegValue};

use windows_sys::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows_sys::Win32::System::Threading::{
    OpenProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
    NORMAL_PRIORITY_CLASS, PROCESS_SET_INFORMATION,
};
use windows_sys::Win32::UI::Shell::{
    SHAppBarMessage, ABM_GETSTATE, ABM_SETSTATE, ABS_ALWAYSONTOP, ABS_AUTOHIDE, APPBARDATA,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
    PostMessageW, SendMessageTimeoutW, SystemParametersInfoW, HWND_BROADCAST, SMTO_ABORTIFHUNG,
    SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETWORKAREA, SPI_SETDESKWALLPAPER, WM_COMMAND,
    WM_SETTINGCHANGE,
};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------- processes

/// Run a PowerShell script hidden, return stdout.
pub fn ps(script: &str) -> Result<String, String> {
    let full = format!(
        "$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.Encoding]::UTF8;{}",
        script
    );
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &full])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { "PowerShell command failed".into() } else { err })
    }
}

/// Run PowerShell and parse JSON output as an array (single objects are wrapped).
pub fn ps_json(script: &str) -> Vec<serde_json::Value> {
    match ps(script) {
        Ok(s) => {
            let s = s.trim();
            if s.is_empty() {
                return vec![];
            }
            match serde_json::from_str::<serde_json::Value>(s) {
                Ok(serde_json::Value::Array(a)) => a,
                Ok(serde_json::Value::Null) => vec![],
                Ok(v) => vec![v],
                Err(_) => vec![],
            }
        }
        Err(_) => vec![],
    }
}

/// Escape a string for use inside single quotes in PowerShell.
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

pub fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if out.status.success() {
        Ok(text)
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { text } else { err })
    }
}

/// Run a full command line through cmd.exe (used for uninstall strings).
pub fn run_cmdline(line: &str) -> Result<i32, String> {
    let status = Command::new("cmd.exe")
        .raw_arg(format!("/C \"{}\"", line))
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    Ok(status.code().unwrap_or(-1))
}

// ---------------------------------------------------------------- registry

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Hive {
    HKCU,
    HKLM,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "t", content = "v")]
pub enum RegVal {
    Dword(u32),
    Str(String),
    Bin(Vec<u8>),
}

fn root(h: Hive) -> RegKey {
    RegKey::predef(match h {
        Hive::HKCU => HKEY_CURRENT_USER,
        Hive::HKLM => HKEY_LOCAL_MACHINE,
    })
}

pub fn reg_get(h: Hive, path: &str, name: &str) -> Option<RegVal> {
    let key = root(h).open_subkey_with_flags(path, KEY_READ).ok()?;
    let raw = key.get_raw_value(name).ok()?;
    let bytes: Vec<u8> = raw.bytes.to_vec();
    match raw.vtype {
        REG_DWORD => {
            if bytes.len() >= 4 {
                Some(RegVal::Dword(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])))
            } else {
                None
            }
        }
        REG_SZ | REG_EXPAND_SZ => key.get_value::<String, _>(name).ok().map(RegVal::Str),
        REG_BINARY => Some(RegVal::Bin(bytes)),
        _ => None,
    }
}

pub fn reg_dword(h: Hive, path: &str, name: &str) -> Option<u32> {
    match reg_get(h, path, name) {
        Some(RegVal::Dword(d)) => Some(d),
        _ => None,
    }
}

pub fn reg_str(h: Hive, path: &str, name: &str) -> Option<String> {
    match reg_get(h, path, name) {
        Some(RegVal::Str(s)) => Some(s),
        _ => None,
    }
}

pub fn reg_set(h: Hive, path: &str, name: &str, v: &RegVal) -> Result<(), String> {
    let (key, _) = root(h).create_subkey(path).map_err(|e| format!("{path}: {e}"))?;
    match v {
        RegVal::Dword(d) => key.set_value(name, d),
        RegVal::Str(s) => key.set_value(name, s),
        RegVal::Bin(b) => key.set_raw_value(
            name,
            &RegValue { bytes: b.clone().into(), vtype: REG_BINARY },
        ),
    }
    .map_err(|e| format!("{path}\\{name}: {e}"))
}

pub fn reg_delete(h: Hive, path: &str, name: &str) -> Result<(), String> {
    match root(h).open_subkey_with_flags(path, KEY_SET_VALUE) {
        Ok(k) => match k.delete_value(name) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        },
        Err(_) => Ok(()),
    }
}

pub fn reg_subkeys(h: Hive, path: &str) -> Vec<String> {
    match root(h).open_subkey_with_flags(path, KEY_READ) {
        Ok(k) => k.enum_keys().filter_map(|x| x.ok()).collect(),
        Err(_) => vec![],
    }
}

/// (name, value-as-string) pairs of a key.
pub fn reg_values(h: Hive, path: &str) -> Vec<(String, String)> {
    match root(h).open_subkey_with_flags(path, KEY_READ) {
        Ok(k) => k
            .enum_values()
            .filter_map(|x| x.ok())
            .map(|(n, _)| {
                let v = k.get_value::<String, _>(&n).unwrap_or_default();
                (n, v)
            })
            .collect(),
        Err(_) => vec![],
    }
}

// ---------------------------------------------------------------- shell

/// Tell Explorer and apps a setting changed so it applies live.
pub fn broadcast(area: &str) {
    let w = wide(area);
    let mut res: usize = 0;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            w.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            2000,
            &mut res,
        );
    }
}

pub fn set_wallpaper(path: &str) -> Result<(), String> {
    let _ = reg_set(Hive::HKCU, r"Control Panel\Desktop", "WallpaperStyle", &RegVal::Str("10".into()));
    let _ = reg_set(Hive::HKCU, r"Control Panel\Desktop", "TileWallpaper", &RegVal::Str("0".into()));
    let w = wide(path);
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            w.as_ptr() as *mut core::ffi::c_void,
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    };
    if ok == 0 {
        Err("Windows refused to set the wallpaper".into())
    } else {
        Ok(())
    }
}

pub fn current_wallpaper() -> String {
    reg_str(Hive::HKCU, r"Control Panel\Desktop", "WallPaper").unwrap_or_default()
}

/// Primary monitor work area in physical pixels (left, top, right, bottom).
pub fn work_area() -> (i32, i32, i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: 1920, bottom: 1040 };
    unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as *mut core::ffi::c_void, 0);
    }
    (r.left, r.top, r.right, r.bottom)
}

fn appbar() -> APPBARDATA {
    let mut d: APPBARDATA = unsafe { std::mem::zeroed() };
    d.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
    d
}

pub fn taskbar_autohide() -> bool {
    let mut d = appbar();
    let state = unsafe { SHAppBarMessage(ABM_GETSTATE, &mut d) };
    (state as u32 & ABS_AUTOHIDE as u32) != 0
}

pub fn set_taskbar_autohide(on: bool) {
    let mut d = appbar();
    let cls = wide("Shell_TrayWnd");
    d.hWnd = unsafe { FindWindowW(cls.as_ptr(), null()) };
    d.lParam = if on { ABS_AUTOHIDE as isize } else { ABS_ALWAYSONTOP as isize };
    unsafe {
        SHAppBarMessage(ABM_SETSTATE, &mut d);
    }
}

fn desktop_defview() -> HWND {
    let defview = wide("SHELLDLL_DefView");
    let progman_cls = wide("Progman");
    let worker_cls = wide("WorkerW");
    unsafe {
        let progman = FindWindowW(progman_cls.as_ptr(), null());
        let mut dv = FindWindowExW(progman, null_mut(), defview.as_ptr(), null());
        if dv.is_null() {
            let mut w: HWND = null_mut();
            loop {
                w = FindWindowExW(null_mut(), w, worker_cls.as_ptr(), null());
                if w.is_null() {
                    break;
                }
                dv = FindWindowExW(w, null_mut(), defview.as_ptr(), null());
                if !dv.is_null() {
                    break;
                }
            }
        }
        dv
    }
}

pub fn desktop_icons_visible() -> bool {
    let lv_cls = wide("SysListView32");
    unsafe {
        let dv = desktop_defview();
        if dv.is_null() {
            return true;
        }
        let lv = FindWindowExW(dv, null_mut(), lv_cls.as_ptr(), null());
        if lv.is_null() {
            return true;
        }
        IsWindowVisible(lv) != 0
    }
}

pub fn set_desktop_icons(visible: bool) {
    if desktop_icons_visible() != visible {
        let dv = desktop_defview();
        if !dv.is_null() {
            unsafe {
                PostMessageW(dv, WM_COMMAND, 0x7402, 0);
            }
        }
    }
}

pub fn foreground_pid() -> u32 {
    unsafe {
        let h = GetForegroundWindow();
        if h.is_null() {
            return 0;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(h, &mut pid);
        pid
    }
}

#[derive(Clone, Copy)]
pub enum Priority {
    High,
    Normal,
    BelowNormal,
}

pub fn set_priority(pid: u32, p: Priority) -> bool {
    unsafe {
        let h = OpenProcess(PROCESS_SET_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let class = match p {
            Priority::High => HIGH_PRIORITY_CLASS,
            Priority::Normal => NORMAL_PRIORITY_CLASS,
            Priority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
        };
        let ok = SetPriorityClass(h, class) != 0;
        CloseHandle(h);
        ok
    }
}

pub fn kill_process(pid: u32) {
    let _ = run("taskkill.exe", &["/PID", &pid.to_string(), "/F"]);
}

// ---------------------------------------------------------------- paths

pub fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

static ELEVATED_ROLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SECURED: std::sync::Once = std::sync::Once::new();

/// Call once at startup in the elevated roles (engine, setup) before anything is written.
pub fn set_elevated_role() {
    ELEVATED_ROLE.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The user's own folder: `%APPDATA%\Nightmoth`. Only the normal-user app writes here.
pub fn user_dir() -> std::path::PathBuf {
    let p = std::path::PathBuf::from(env("APPDATA")).join("Nightmoth");
    let _ = std::fs::create_dir_all(&p);
    p
}

/// The engine's folder: `%ProgramData%\Nightmoth`. Only admins can write to it, everyone can read it,
/// so nothing a normal program writes can steer the elevated engine.
pub fn engine_dir() -> std::path::PathBuf {
    let base = env("ProgramData");
    let base = if base.is_empty() { r"C:\ProgramData".to_string() } else { base };
    std::path::PathBuf::from(base).join("Nightmoth")
}

/// Create the engine folder and lock it down: owner Administrators, write access for
/// SYSTEM and Administrators only, read for Users. A junction planted there is removed first.
fn secure_engine_dir(p: &std::path::Path) {
    if let Ok(m) = std::fs::symlink_metadata(p) {
        if m.file_type().is_symlink() {
            let _ = std::fs::remove_dir(p);
            let _ = std::fs::remove_file(p);
        }
    }
    let _ = std::fs::create_dir_all(p);
    let dir = p.to_string_lossy().to_string();
    let _ = run("icacls.exe", &[&dir, "/setowner", "*S-1-5-32-544", "/T", "/C", "/L", "/Q"]);
    let _ = run(
        "icacls.exe",
        &[&dir, "/inheritance:r", "/grant:r", "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F", "*S-1-5-32-545:(OI)(CI)RX", "/T", "/C", "/L", "/Q"],
    );
}

/// Where this process keeps its files: the locked engine folder for the elevated
/// roles, the user's AppData folder for the app.
pub fn data_dir() -> std::path::PathBuf {
    if ELEVATED_ROLE.load(std::sync::atomic::Ordering::Relaxed) {
        let p = engine_dir();
        SECURED.call_once(|| secure_engine_dir(&p));
        let _ = std::fs::create_dir_all(&p);
        p
    } else {
        user_dir()
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Read one of the engine's state files. Before 0.3 they lived in the user's AppData:
/// if the new copy is missing, the old one is copied over once.
pub fn read_state_file(name: &str) -> Option<String> {
    let p = data_dir().join(name);
    if let Ok(s) = std::fs::read_to_string(&p) {
        return Some(s);
    }
    let old = std::path::PathBuf::from(env("APPDATA")).join("Nightmoth").join(name);
    let m = std::fs::symlink_metadata(&old).ok()?;
    if !m.is_file() || m.len() > 32 * 1024 * 1024 {
        return None;
    }
    let s = std::fs::read_to_string(&old).ok()?;
    let _ = std::fs::write(&p, &s);
    Some(s)
}

/// (name, raw bytes) of every value in a key.
pub fn reg_values_raw(h: Hive, path: &str) -> Vec<(String, Vec<u8>)> {
    match root(h).open_subkey_with_flags(path, KEY_READ) {
        Ok(k) => k.enum_values().filter_map(|x| x.ok()).map(|(n, v)| (n, v.bytes.to_vec())).collect(),
        Err(_) => vec![],
    }
}

// ---------------------------------------------------------------- power, idle, efficiency

/// Seconds since the last keyboard/mouse input in this session.
pub fn idle_secs() -> u64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if GetLastInputInfo(&mut li) == 0 {
            return 0;
        }
        (GetTickCount().wrapping_sub(li.dwTime) / 1000) as u64
    }
}

/// (has a battery, running on battery, battery percent)
pub fn power_source() -> (bool, bool, u8) {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut s) } == 0 {
        return (false, false, 100);
    }
    let has_battery = s.BatteryFlag != 128 && s.BatteryFlag != 255;
    (has_battery, has_battery && s.ACLineStatus == 0, s.BatteryLifePercent.min(100))
}

/// Windows 11 "Efficiency mode" (what Task Manager does): EcoQoS + idle priority.
pub fn set_efficiency(pid: u32, on: bool) -> bool {
    use windows_sys::Win32::System::Threading::{
        ProcessPowerThrottling, SetProcessInformation, IDLE_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    };
    unsafe {
        let h = OpenProcess(PROCESS_SET_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let st = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: if on { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 },
        };
        let ok = SetProcessInformation(
            h,
            ProcessPowerThrottling,
            &st as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        ) != 0;
        SetPriorityClass(h, if on { IDLE_PRIORITY_CLASS } else { NORMAL_PRIORITY_CLASS });
        CloseHandle(h);
        ok
    }
}

/// Free and total bytes of the system drive.
pub fn system_drive_space() -> (u64, u64) {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_lowercase();
    for d in sysinfo::Disks::new_with_refreshed_list().list() {
        if d.mount_point().to_string_lossy().to_lowercase().starts_with(&drive) {
            return (d.available_space(), d.total_space());
        }
    }
    (0, 0)
}

/// Delete a whole registry key (and everything under it). Ok if it doesn't exist.
pub fn reg_delete_tree(h: Hive, path: &str) -> Result<(), String> {
    match root(h).delete_subkey_all(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
