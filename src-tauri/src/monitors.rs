//! A different wallpaper on every screen (app side). The main screen's wallpaper still
//! drives the theme; the others are set on their own and kept after theme changes.

use serde_json::{json, Value};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{DesktopWallpaper, IDesktopWallpaper};

fn api() -> Result<IDesktopWallpaper, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL).map_err(|e| e.to_string())
    }
}

fn take(p: PWSTR) -> String {
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
    s
}

/// Every screen: id, size, position, current wallpaper, main screen or not.
pub fn list() -> Result<Vec<Value>, String> {
    let dw = api()?;
    let mut out = vec![];
    unsafe {
        let n = dw.GetMonitorDevicePathCount().map_err(|e| e.to_string())?;
        for i in 0..n {
            let Ok(p) = dw.GetMonitorDevicePathAt(i) else { continue };
            let id = take(p);
            let h = HSTRING::from(id.as_str());
            let Ok(r) = dw.GetMonitorRECT(PCWSTR(h.as_ptr())) else { continue };
            let wall = dw.GetWallpaper(PCWSTR(h.as_ptr())).map(take).unwrap_or_default();
            out.push(json!({
                "id": id, "wallpaper": wall, "primary": r.left == 0 && r.top == 0,
                "x": r.left, "y": r.top, "w": r.right - r.left, "h": r.bottom - r.top,
            }));
        }
    }
    Ok(out)
}

pub fn set(id: &str, path: &str) -> Result<(), String> {
    let dw = api()?;
    let m = HSTRING::from(id);
    let p = HSTRING::from(path);
    unsafe { dw.SetWallpaper(PCWSTR(m.as_ptr()), PCWSTR(p.as_ptr())).map_err(|e| e.to_string()) }
}

/// After the theme sets one wallpaper everywhere, put the other screens' own pictures back.
pub fn reapply(walls: &serde_json::Map<String, Value>) {
    let Ok(screens) = list() else { return };
    for s in screens {
        if s["primary"].as_bool().unwrap_or(false) {
            continue;
        }
        let id = s["id"].as_str().unwrap_or("");
        if let Some(w) = walls.get(id).and_then(|w| w.as_str()) {
            if !w.is_empty() && s["wallpaper"].as_str() != Some(w) && std::path::Path::new(w).exists() {
                let _ = set(id, w);
            }
        }
    }
}
