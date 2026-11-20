//! Desktop widgets (app side): clock, calendar, system monitor and Nightmoth's week,
//! drawn in one see-through window that sits on the desktop behind every other window.
//!
//! The engine decides what they look like (accent from the wallpaper) and where they
//! go (the calmest part of the wallpaper); the app owns the window and live stats.

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::{Disks, Networks, System};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::log::log;
use crate::state::lock;

/// Size of the widget column in CSS pixels, reported by the page.
static SIZE: Mutex<(f64, f64)> = Mutex::new((320.0, 420.0));
/// The page has loaded and listens for events.
static READY: AtomicBool = AtomicBool::new(false);
/// Ask the loop to send the state again (page reloaded).
static RESEND: AtomicBool = AtomicBool::new(true);

pub fn page_ready() {
    READY.store(true, Ordering::Relaxed);
    RESEND.store(true, Ordering::Relaxed);
}

pub fn set_size(w: f64, h: f64) {
    *lock(&SIZE) = (w.clamp(120.0, 900.0), h.clamp(60.0, 1400.0));
}

fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    READY.store(false, Ordering::Relaxed);
    let w = WebviewWindowBuilder::new(app, "widgets", WebviewUrl::App("widgets.html".into()))
        .title("Nightmoth widgets")
        .inner_size(320.0, 420.0)
        .decorations(false)
        .transparent(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .shadow(false)
        .always_on_bottom(true)
        .visible(false)
        .build()?;
    // never in the way: clicks go through to the desktop, no Alt+Tab entry, never takes focus
    let _ = w.set_ignore_cursor_events(true);
    if let Ok(h) = w.hwnd() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW};
        let hwnd: windows_sys::Win32::Foundation::HWND = h.0 as _;
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) as isize);
        }
    }
    log("widgets window created");
    Ok(w)
}

/// Put the widget column in one of the 9 spots of the primary screen's work area.
fn place(w: &WebviewWindow, cell: usize, last: &mut (i32, i32, u32, u32)) {
    let (l, t, r, b) = crate::win::work_area();
    let scale = w.scale_factor().unwrap_or(1.0);
    let (cw, ch) = *lock(&SIZE);
    let (pw, ph) = ((cw * scale).round() as i32, (ch * scale).round() as i32);
    let m = (40.0 * scale) as i32;
    let x = match cell % 3 {
        0 => l + m,
        1 => l + (r - l - pw) / 2,
        _ => r - pw - m,
    };
    let y = match cell / 3 {
        0 => t + m,
        1 => t + (b - t - ph) / 2,
        _ => b - ph - m,
    };
    let next = (x.max(l), y.max(t), pw.max(1) as u32, ph.max(1) as u32);
    if next != *last {
        let _ = w.set_size(PhysicalSize::new(next.2, next.3));
        let _ = w.set_position(PhysicalPosition::new(next.0, next.1));
        *last = next;
    }
}

struct Sampler {
    sys: System,
    rx: u64,
    tx: u64,
    at: Instant,
    disk: (u64, u64),
    disk_at: Option<Instant>,
}

impl Sampler {
    fn new() -> Self {
        Sampler { sys: System::new(), rx: 0, tx: 0, at: Instant::now(), disk: (0, 0), disk_at: None }
    }

    fn sample(&mut self) -> Value {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        let nets = Networks::new_with_refreshed_list();
        let (rx, tx) = nets.list().iter().fold((0u64, 0u64), |(r, t), (_, d)| (r + d.total_received(), t + d.total_transmitted()));
        let secs = self.at.elapsed().as_secs_f64().max(0.5);
        let (down, up) = if self.rx == 0 { (0.0, 0.0) } else { (rx.saturating_sub(self.rx) as f64 / secs, tx.saturating_sub(self.tx) as f64 / secs) };
        self.rx = rx;
        self.tx = tx;
        self.at = Instant::now();
        if self.disk_at.map(|t| t.elapsed() > Duration::from_secs(30)).unwrap_or(true) {
            let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_lowercase();
            for d in Disks::new_with_refreshed_list().list() {
                if d.mount_point().to_string_lossy().to_lowercase().starts_with(&drive) {
                    self.disk = (d.total_space() - d.available_space(), d.total_space());
                }
            }
            self.disk_at = Some(Instant::now());
        }
        json!({
            "cpu": self.sys.global_cpu_usage(),
            "ram_used": self.sys.used_memory(),
            "ram_total": self.sys.total_memory(),
            "disk_used": self.disk.0,
            "disk_total": self.disk.1,
            "down": down,
            "up": up,
            "uptime": System::uptime(),
        })
    }
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        let mut sampler = Sampler::new();
        let mut state: Option<Value> = None;
        let mut sent: Option<Value> = None;
        let mut last_place = (0, 0, 0, 0);
        let mut tick: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(1000));
            tick += 1;
            if tick % 3 == 1 {
                if let Ok(st) = crate::ipc::call_quiet("widgets_state", json!({})) {
                    state = Some(st);
                }
            }
            let Some(st) = state.clone() else { continue };
            crate::taskbar::set_wanted(st["taskbar"].as_str().unwrap_or("normal"), st["taskbar_accent"].as_str().unwrap_or("#B3141C"));
            let wanted = st["on"].as_bool().unwrap_or(false)
                && !st["boosting"].as_bool().unwrap_or(false)
                && st["widgets"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
            let existing = app.get_webview_window("widgets");
            if !wanted {
                if let Some(w) = existing {
                    let _ = w.hide();
                }
                continue;
            }
            let w = match existing {
                Some(w) => w,
                None => match create(&app) {
                    Ok(w) => {
                        last_place = (0, 0, 0, 0);
                        sent = None;
                        w
                    }
                    Err(e) => {
                        log(&format!("widgets window failed: {e}"));
                        std::thread::sleep(Duration::from_secs(30));
                        continue;
                    }
                },
            };
            if !READY.load(Ordering::Relaxed) {
                continue;
            }
            if RESEND.swap(false, Ordering::Relaxed) || sent.as_ref() != Some(&st) {
                let _ = app.emit_to("widgets", "widgets-state", &st);
                sent = Some(st.clone());
            }
            if tick % 2 == 0 {
                let _ = app.emit_to("widgets", "widgets-stats", sampler.sample());
            }
            place(&w, st["cell"].as_u64().unwrap_or(2) as usize, &mut last_place);
            // "Show desktop" (Win+D) can minimise it: bring it back without taking focus
            if w.is_minimized().unwrap_or(false) {
                let _ = w.unminimize();
            }
            if !w.is_visible().unwrap_or(false) {
                let _ = w.show();
            }
        }
    });
}
