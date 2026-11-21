//! See-through taskbar (app side, normal user rights): the bar's background goes
//! clear or tinted glass so only the app icons show. Explorer resets the effect when
//! the taskbar changes (Start opens, a window maximises…), so it's re-applied constantly.
//!
//! Uses the window-composition API that TranslucentTB-style tools use. On newer
//! Windows 11 builds the taskbar paints its own background on top, so "clear" can be
//! partial there.

use std::sync::Mutex;
use std::time::Duration;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW};

use crate::state::lock;
use crate::win::wide;

/// (style, accent as 0xRRGGBB) — set by the widgets loop from the engine's config.
static WANTED: Mutex<(String, u32)> = Mutex::new((String::new(), 0));

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    gradient: u32,
    animation: u32,
}

#[repr(C)]
struct CompositionData {
    attribute: u32,
    data: *mut core::ffi::c_void,
    size: usize,
}

type SetComposition = unsafe extern "system" fn(HWND, *mut CompositionData) -> i32;

const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_DISABLED: u32 = 0;
const ACCENT_TRANSPARENT: u32 = 2;
const ACCENT_ACRYLIC: u32 = 4;

pub fn set_wanted(style: &str, accent_hex: &str) {
    let rgb = u32::from_str_radix(accent_hex.trim_start_matches('#'), 16).unwrap_or(0xB3141C);
    *lock(&WANTED) = (style.to_string(), rgb);
}

fn api() -> Option<SetComposition> {
    let user32 = wide("user32.dll");
    unsafe {
        let h = GetModuleHandleW(user32.as_ptr());
        if h.is_null() {
            return None;
        }
        let f = GetProcAddress(h, b"SetWindowCompositionAttribute\0".as_ptr())?;
        Some(std::mem::transmute::<unsafe extern "system" fn() -> isize, SetComposition>(f))
    }
}

fn taskbars() -> Vec<HWND> {
    let main = wide("Shell_TrayWnd");
    let second = wide("Shell_SecondaryTrayWnd");
    let mut out = vec![];
    unsafe {
        let h = FindWindowW(main.as_ptr(), std::ptr::null());
        if !h.is_null() {
            out.push(h);
        }
        let mut w: HWND = std::ptr::null_mut();
        loop {
            w = FindWindowExW(std::ptr::null_mut(), w, second.as_ptr(), std::ptr::null());
            if w.is_null() {
                break;
            }
            out.push(w);
        }
    }
    out
}

fn apply(f: SetComposition, state: u32, gradient: u32) {
    for h in taskbars() {
        let mut policy = AccentPolicy { state, flags: 2, gradient, animation: 0 };
        let mut data = CompositionData {
            attribute: WCA_ACCENT_POLICY,
            data: &mut policy as *mut AccentPolicy as *mut core::ffi::c_void,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        unsafe {
            f(h, &mut data);
        }
    }
}

// ------------------------------------------------------------------ Windows 11 helper

/// Must match the helper DLL.
const CLSID_TAP: windows_sys::core::GUID = windows_sys::core::GUID::from_u128(0x6b3a2b1e_8f4c_4e3b_9a51_4e49474854d0);

type InitXamlDiag = unsafe extern "system" fn(
    *const u16,
    u32,
    *const u16,
    *const u16,
    windows_sys::core::GUID,
    *const u16,
) -> windows_sys::core::HRESULT;

fn windows_build() -> u32 {
    crate::win::reg_str(crate::win::Hive::HKLM, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuildNumber")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn explorer_pid() -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let cls = wide("Shell_TrayWnd");
    unsafe {
        let h = FindWindowW(cls.as_ptr(), std::ptr::null());
        if h.is_null() {
            return 0;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        pid
    }
}

/// Tell the helper (inside Explorer) whether to hide the taskbar background.
fn set_flag(on: bool) {
    let _ = crate::win::reg_set(crate::win::Hive::HKCU, r"Software\Nightmoth", "TaskbarClear", &crate::win::RegVal::Dword(on as u32));
}

fn inject(dll: &std::path::Path, pid: u32, n: u32) -> Result<(), String> {
    let lib = wide("Windows.UI.Xaml.dll");
    unsafe {
        let h = windows_sys::Win32::System::LibraryLoader::LoadLibraryW(lib.as_ptr());
        if h.is_null() {
            return Err("XAML not available".into());
        }
        let f = GetProcAddress(h, b"InitializeXamlDiagnosticsEx\0".as_ptr()).ok_or("no InitializeXamlDiagnosticsEx")?;
        let f: InitXamlDiag = std::mem::transmute(f);
        // the connection name TranslucentTB uses; a fresh number for every attempt
        let endpoint = wide(&format!("VisualDiagConnection{n}"));
        let path = wide(&dll.to_string_lossy());
        let hr = f(endpoint.as_ptr(), pid, std::ptr::null(), path.as_ptr(), CLSID_TAP, std::ptr::null());
        if hr < 0 {
            return Err(format!("0x{:08X}", hr as u32));
        }
    }
    Ok(())
}

/// What the Windows 11 helper is doing, shown on the Look page.
static STATUS: Mutex<String> = Mutex::new(String::new());

fn status(msg: &str) {
    let mut s = lock(&STATUS);
    if *s != msg {
        crate::log::log(&format!("taskbar helper: {msg}"));
        *s = msg.to_string();
    }
}

pub fn helper_status() -> String {
    let s = lock(&STATUS).clone();
    if s.is_empty() {
        "starting".into()
    } else {
        s
    }
}

/// Keeps the helper loaded in Explorer (it's reloaded when Explorer restarts) while the
/// taskbar should be clear. Explorer may not have its taskbar ready yet, so a failed load
/// is retried a few times. If Explorer restarts right after a load twice, it stops for good.
fn helper_loop(dll: std::path::PathBuf) {
    if windows_build() < 22000 {
        status("not needed on Windows 10 — the basic effect makes it clear");
        return;
    }
    if !dll.exists() {
        status("helper file missing — reinstall Nightmoth");
        return;
    }
    std::thread::spawn(move || {
        let mut loaded_pid = 0u32;
        let mut loaded_at = std::time::Instant::now();
        let mut tries_pid = 0u32;
        let mut tries = 0u32;
        let mut attempt = 0u32;
        loop {
            std::thread::sleep(Duration::from_secs(3));
            let (style, _) = lock(&WANTED).clone();
            let want = style == "clear" || style == "tint";
            set_flag(want);
            if !want {
                if loaded_pid == 0 {
                    status("off — taskbar set to Windows default");
                }
                continue;
            }
            let Ok(c) = crate::ipc::call_quiet("get_config", serde_json::json!({})) else { continue };
            if !c["taskbar_helper"].as_bool().unwrap_or(true) {
                status("switched off");
                continue;
            }
            if c["taskbar_helper_crashes"].as_u64().unwrap_or(0) >= 2 {
                status("paused — Explorer restarted right after it loaded");
                continue;
            }
            let pid = explorer_pid();
            if pid == 0 {
                status("waiting for the taskbar");
                continue;
            }
            if pid == loaded_pid {
                continue;
            }
            // Explorer was replaced soon after we loaded the helper: count it as a crash
            if loaded_pid != 0 && loaded_at.elapsed() < Duration::from_secs(90) {
                let n = c["taskbar_helper_crashes"].as_u64().unwrap_or(0) + 1;
                let _ = crate::ipc::call_quiet("set_config", serde_json::json!({ "patch": { "taskbar_helper_crashes": n } }));
                if n >= 2 {
                    crate::ui_toast::local("Invisible taskbar paused", "Explorer didn't like the helper on this PC — using the basic effect");
                    loaded_pid = pid;
                    continue;
                }
            }
            if pid != tries_pid {
                tries_pid = pid;
                tries = 0;
                // a fresh Explorer needs a moment to build its taskbar
                std::thread::sleep(Duration::from_secs(5));
            }
            if tries >= 6 {
                continue; // gave up on this Explorer; tries again after it restarts
            }
            tries += 1;
            attempt += 1;
            match inject(&dll, pid, attempt) {
                Ok(_) => {
                    status("active — taskbar background hidden");
                    loaded_pid = pid;
                    loaded_at = std::time::Instant::now();
                }
                Err(e) => {
                    status(&format!("couldn't load into Explorer ({e}), try {tries} of 6"));
                    std::thread::sleep(Duration::from_secs(10));
                }
            }
        }
    });
}

pub fn start(helper_dll: Option<std::path::PathBuf>) {
    if let Some(dll) = helper_dll {
        helper_loop(dll);
    }
    std::thread::spawn(|| {
        let Some(f) = api() else {
            crate::log::log("taskbar: composition API not available");
            return;
        };
        let mut active = false;
        loop {
            let (style, rgb) = lock(&WANTED).clone();
            // gradient colour is ABGR
            let bgr = ((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF);
            match style.as_str() {
                "clear" => {
                    apply(f, ACCENT_TRANSPARENT, 0x0000_0000);
                    active = true;
                }
                "tint" => {
                    apply(f, ACCENT_ACRYLIC, 0x2E00_0000 | bgr);
                    active = true;
                }
                _ => {
                    if active {
                        // hand the taskbar back to Windows
                        apply(f, ACCENT_DISABLED, 0);
                        active = false;
                    }
                    std::thread::sleep(Duration::from_millis(900));
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
}
