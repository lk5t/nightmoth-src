//! Engine side of the pop-ups: the engine queues them, the app (which owns the
//! pop-up window) collects and shows them, and reports clicks back.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use crate::state::{cfg, core, lock, Action};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Toast {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub action: Option<String>,
    pub ms: u64,
}

static QUEUE: Mutex<VecDeque<Toast>> = Mutex::new(VecDeque::new());
static SHOWN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Kinds that ignore quiet mode because they let you stop something.
const ALWAYS: &[&str] = &["remove"];

fn next_id() -> String {
    format!("t{}{}", crate::win::now(), rand::random::<u32>())
}

/// Queue a pop-up. `kind` is used for per-type on/off settings.
pub fn show(kind: &str, title: &str, body: &str) {
    push(kind, title, body, None);
}

/// Queue a pop-up with a button. Returns its id (used for cancel windows).
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
        // drop unused actions after a while
        let drop_id = id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(300));
            lock(&core().actions).remove(&drop_id);
        });
    }
    let mut q = lock(&QUEUE);
    q.push_back(Toast {
        id: id.clone(),
        kind: kind.into(),
        title: title.into(),
        body: body.into(),
        action: label,
        ms: c.toast_ms.clamp(2000, 15000),
    });
    // if the app isn't running nobody collects them: keep only recent ones
    while q.len() > 20 {
        q.pop_front();
    }
    id
}

/// The app collects queued pop-ups.
pub fn take() -> Vec<Toast> {
    lock(&QUEUE).drain(..).collect()
}

/// The app reports a pop-up is on screen.
pub fn mark_shown(id: &str) {
    let mut shown = lock(&SHOWN);
    shown.push(id.to_string());
    if shown.len() > 100 {
        shown.remove(0);
    }
}

/// Block until a pop-up is on screen (or the timeout passes). Used by cancel windows.
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

/// A pop-up's button was clicked.
pub fn action(id: &str) {
    let f = lock(&core().actions).remove(id);
    if let Some(f) = f {
        std::thread::spawn(f);
    }
}
