//! Razer-Cortex-style pop-ups, bottom right, one at a time.

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{Emitter, Manager, PhysicalPosition};

use crate::state::{app, cfg, core, lock, Action};

#[derive(Serialize, Clone, Debug)]
pub struct Toast {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub action: Option<String>,
    pub ms: u64,
}

static QUEUE: Mutex<VecDeque<Toast>> = Mutex::new(VecDeque::new());
static WAKE: OnceLock<Mutex<Sender<()>>> = OnceLock::new();
static DONE: OnceLock<Mutex<Sender<()>>> = OnceLock::new();
static SHOWN: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Set once the pop-up window's page has loaded and is listening.
pub static READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Block until a toast is on screen (or the timeout passes). Used by cancel windows.
pub fn wait_shown(id: &str, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if lock(&SHOWN).iter().any(|x| x == id) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

/// Kinds that ignore quiet mode because they let you stop something.
const ALWAYS: &[&str] = &["remove"];

pub fn start() {
    let (wake_tx, wake_rx) = channel::<()>();
    let (done_tx, done_rx) = channel::<()>();
    let _ = WAKE.set(Mutex::new(wake_tx));
    let _ = DONE.set(Mutex::new(done_tx));
    std::thread::spawn(move || worker(wake_rx, done_rx));
}

fn next_id() -> String {
    format!("t{}{}", crate::win::now(), rand::random::<u32>())
}

/// Show a toast. `kind` is used for per-type on/off settings.
pub fn show(kind: &str, title: &str, body: &str) {
    push(kind, title, body, None);
}

/// Show a toast with a button. Returns the toast id (used for cancel windows).
pub fn show_action(kind: &str, title: &str, body: &str, label: &str, action: Action) -> String {
    push(kind, title, body, Some((label.to_string(), action)))
}

fn push(kind: &str, title: &str, body: &str, action: Option<(String, Action)>) -> String {
    let c = cfg();
    let id = next_id();
    let forced = ALWAYS.contains(&kind);
    if !forced && (!c.toast_enabled || c.toast_quiet || c.toast_off.iter().any(|k| k == kind)) {
        return id;
    }
    let label = action.as_ref().map(|(l, _)| l.clone());
    if let Some((_, f)) = action {
        lock(&core().actions).insert(id.clone(), f);
    }
    lock(&QUEUE).push_back(Toast {
        id: id.clone(),
        kind: kind.into(),
        title: title.into(),
        body: body.into(),
        action: label,
        ms: c.toast_ms.clamp(2000, 15000),
    });
    if let Some(w) = WAKE.get() {
        let _ = lock(w).send(());
    }
    id
}

/// Called by the toast window when a toast finished (timer ran out or dismissed).
pub fn done() {
    if let Some(d) = DONE.get() {
        let _ = lock(d).send(());
    }
}

/// Called when the toast's button is clicked.
pub fn action(id: &str) {
    let f = lock(&core().actions).remove(id);
    if let Some(f) = f {
        std::thread::spawn(f);
    }
}

fn position(win: &tauri::WebviewWindow) {
    let (_, _, right, bottom) = crate::win::work_area();
    let scale = win.scale_factor().unwrap_or(1.0);
    let size = win.outer_size().map(|s| (s.width as i32, s.height as i32)).unwrap_or((380, 104));
    let margin = (14.0 * scale) as i32;
    let _ = win.set_position(PhysicalPosition::new(right - size.0 - margin, bottom - size.1 - margin));
}

fn worker(wake: Receiver<()>, done: Receiver<()>) {
    loop {
        let next = lock(&QUEUE).pop_front();
        let Some(t) = next else {
            if let Some(win) = app().and_then(|a| a.get_webview_window("toast")) {
                let _ = win.hide();
            }
            // wait until something is queued
            let _ = wake.recv();
            continue;
        };
        // wait (up to 15 s) for the pop-up window to exist and load
        for _ in 0..150 {
            if READY.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // drain stale done signals
        while done.try_recv().is_ok() {}
        if let Some(a) = app() {
            if let Some(win) = a.get_webview_window("toast") {
                position(&win);
                let _ = win.show();
                let _ = win.set_always_on_top(true);
            }
            let _ = a.emit_to("toast", "toast", t.clone());
        }
        {
            let mut shown = lock(&SHOWN);
            shown.push(t.id.clone());
            if shown.len() > 50 {
                shown.remove(0);
            }
        }
        // the window reports back when done; hovering can extend it
        let _ = done.recv_timeout(Duration::from_millis(t.ms + 60_000));
        // drop an unused action after a while (cancel windows rely on this)
        let id = t.id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(30));
            lock(&core().actions).remove(&id);
        });
        std::thread::sleep(Duration::from_millis(250));
    }
}
