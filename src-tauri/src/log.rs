//! Startup/crash logging to %APPDATA%\Nightmoth\nightmoth.log so failures are visible.

use std::io::Write;

use crate::win::{data_dir, wide};
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

static ROLE: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// "app", "engine" or "setup" — shown on every log line.
pub fn set_role(r: &'static str) {
    let _ = ROLE.set(r);
}

pub fn log(msg: &str) {
    let role = ROLE.get().copied().unwrap_or("?");
    let line = format!("{} [{}] {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), role, msg);
    let p = data_dir().join("nightmoth.log");
    // keep the file small
    if std::fs::metadata(&p).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&p);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Show an error box (skipped in CI smoke tests so nothing blocks).
pub fn error_box(text: &str) {
    if std::env::var("NIGHTMOTH_NO_DIALOG").is_ok() {
        return;
    }
    let t = wide(text);
    let c = wide("Nightmoth");
    unsafe {
        MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("PANIC: {info}");
        log(&msg);
        let _ = std::fs::write(data_dir().join("crash.log"), &msg);
        // only show a box for the app's main thread (background/engine panics are logged)
        if std::thread::current().name() == Some("main") && ROLE.get().copied() == Some("app") {
            error_box(&format!(
                "Nightmoth crashed while starting.\n\n{info}\n\nDetails were saved to %APPDATA%\\Nightmoth\\crash.log"
            ));
        }
    }));
}
