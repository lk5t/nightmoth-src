//! Things the elevated engine must NOT do with admin rights: running uninstallers that
//! a normal program could have registered (per-user apps), and reopening apps closed
//! for a game boost. The engine queues them; the normal-user app runs them.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::state::lock;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Job {
    pub id: String,
    /// "cmd" = run a command line through cmd.exe, "launch" = start an exe
    pub kind: String,
    pub line: String,
}

static QUEUE: Mutex<VecDeque<Job>> = Mutex::new(VecDeque::new());
static DONE: Mutex<Option<HashMap<String, Result<i32, String>>>> = Mutex::new(None);
static LAST_POLL: AtomicI64 = AtomicI64::new(0);

// ------------------------------------------------------------------ engine side

/// Is the app running and collecting jobs?
pub fn app_connected() -> bool {
    crate::win::now() - LAST_POLL.load(Ordering::Relaxed) < 10
}

/// Run something with the user's normal rights. With `wait`, block until it finishes.
pub fn run(kind: &str, line: &str, wait: Option<Duration>) -> Result<i32, String> {
    if !app_connected() {
        return Err("Open Nightmoth (the window app) and try again".into());
    }
    let id = format!("j{}{}", crate::win::now(), rand::random::<u32>());
    lock(&QUEUE).push_back(Job { id: id.clone(), kind: kind.into(), line: line.into() });
    let Some(max) = wait else { return Ok(0) };
    let start = Instant::now();
    while start.elapsed() < max {
        if let Some(r) = lock(&DONE).as_mut().and_then(|m| m.remove(&id)) {
            return r;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Err("Timed out waiting for the uninstaller".into())
}

/// The app collects queued jobs.
pub fn take() -> Vec<Job> {
    LAST_POLL.store(crate::win::now(), Ordering::Relaxed);
    lock(&QUEUE).drain(..).collect()
}

/// The app reports a finished job.
pub fn done(id: &str, code: i32, error: &str) {
    let mut d = lock(&DONE);
    let m = d.get_or_insert_with(HashMap::new);
    m.insert(id.to_string(), if error.is_empty() { Ok(code) } else { Err(error.to_string()) });
    if m.len() > 200 {
        m.clear();
    }
}

// ------------------------------------------------------------------ app side

/// Poll the engine for jobs and run them as the normal user.
pub fn start_app_worker() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(1000));
        let Ok(v) = crate::ipc::call_quiet("user_jobs_take", json!({})) else { continue };
        let Ok(jobs) = serde_json::from_value::<Vec<Job>>(v) else { continue };
        for job in jobs {
            std::thread::spawn(move || {
                crate::log::log(&format!("app: running {} job", job.kind));
                let (code, error) = match job.kind.as_str() {
                    "cmd" => match crate::win::run_cmdline(&job.line) {
                        Ok(c) => (c, String::new()),
                        Err(e) => (-1, e),
                    },
                    "launch" => match std::process::Command::new(&job.line).spawn() {
                        Ok(_) => (0, String::new()),
                        Err(e) => (-1, e.to_string()),
                    },
                    other => (-1, format!("unknown job {other}")),
                };
                let _ = crate::ipc::call_quiet("user_job_done", json!({ "id": job.id, "code": code, "error": error }));
            });
        }
    });
}
