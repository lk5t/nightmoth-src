//! App side of the pop-ups: collects them from the engine and shows them
//! bottom-right one at a time in the transparent pop-up window.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

use crate::state::lock;
use crate::toast::Toast;

static QUEUE: Mutex<VecDeque<Toast>> = Mutex::new(VecDeque::new());
static DONE: OnceLock<Mutex<Sender<()>>> = OnceLock::new();
/// Set once the pop-up window's page has loaded and is listening.
pub static READY: AtomicBool = AtomicBool::new(false);

pub fn start(app: AppHandle) {
    let (done_tx, done_rx) = channel::<()>();
    let _ = DONE.set(Mutex::new(done_tx));
    let a2 = app.clone();
    std::thread::spawn(move || worker(a2, done_rx));
    // poll the engine for new pop-ups
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(700));
        if let Ok(v) = crate::ipc::call_quiet("toast_take", serde_json::json!({})) {
            if let Ok(list) = serde_json::from_value::<Vec<Toast>>(v) {
                lock(&QUEUE).extend(list);
            }
        }
    });
    let _ = app;
}

/// A pop-up made by the app itself (e.g. when the engine can't be reached).
pub fn local(title: &str, body: &str) {
    lock(&QUEUE).push_back(Toast {
        id: format!("local{}", rand::random::<u32>()),
        kind: "local".into(),
        title: title.into(),
        body: body.into(),
        action: None,
        ms: 6000,
    });
}

pub fn done() {
    if let Some(d) = DONE.get() {
        let _ = lock(d).send(());
    }
}

fn position(win: &tauri::WebviewWindow) {
    let (_, _, right, bottom) = crate::win::work_area();
    let scale = win.scale_factor().unwrap_or(1.0);
    let size = win.outer_size().map(|s| (s.width as i32, s.height as i32)).unwrap_or((380, 104));
    let margin = (14.0 * scale) as i32;
    let _ = win.set_position(PhysicalPosition::new(right - size.0 - margin, bottom - size.1 - margin));
}

fn worker(app: AppHandle, done: Receiver<()>) {
    loop {
        let next = lock(&QUEUE).pop_front();
        let Some(t) = next else {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        // wait (up to 20 s) for the pop-up window to load
        for _ in 0..200 {
            if READY.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        while done.try_recv().is_ok() {}
        if let Some(win) = app.get_webview_window("toast") {
            position(&win);
            let _ = win.show();
            let _ = win.set_always_on_top(true);
        }
        let _ = app.emit_to("toast", "toast", t.clone());
        if !t.id.starts_with("local") {
            let _ = crate::ipc::call_quiet("toast_shown", serde_json::json!({ "id": t.id }));
        }
        let _ = done.recv_timeout(Duration::from_millis(t.ms + 60_000));
        std::thread::sleep(Duration::from_millis(250));
        if lock(&QUEUE).is_empty() {
            if let Some(win) = app.get_webview_window("toast") {
                let _ = win.hide();
            }
        }
    }
}
