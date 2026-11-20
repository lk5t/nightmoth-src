//! Global app state shared by commands and background threads.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::changelog::ChangeLog;
use crate::config::Config;

pub type Action = Box<dyn FnOnce() + Send>;

pub struct Core {
    pub cfg: Mutex<Config>,
    pub log: Mutex<ChangeLog>,
    pub actions: Mutex<HashMap<String, Action>>,
    pub cancelled: Mutex<HashSet<String>>,
    pub boosting: AtomicBool,
    pub sys: Mutex<sysinfo::System>,
    pub started: i64,
}

static CORE: OnceLock<Core> = OnceLock::new();
/// Only the engine (elevated background process) owns the core state.
pub fn init() {
    let _ = CORE.set(Core {
        cfg: Mutex::new(Config::load()),
        log: Mutex::new(ChangeLog::load()),
        actions: Mutex::new(HashMap::new()),
        cancelled: Mutex::new(HashSet::new()),
        boosting: AtomicBool::new(false),
        sys: Mutex::new(sysinfo::System::new()),
        started: crate::win::now(),
    });
}

pub fn core() -> &'static Core {
    CORE.get().expect("core not initialised")
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Snapshot of the config.
pub fn cfg() -> Config {
    lock(&core().cfg).clone()
}

/// Mutate the config and save it.
pub fn with_cfg<R>(f: impl FnOnce(&mut Config) -> R) -> R {
    let mut c = lock(&core().cfg);
    let r = f(&mut c);
    c.save();
    r
}
