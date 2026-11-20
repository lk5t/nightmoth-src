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

pub fn start() {
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
