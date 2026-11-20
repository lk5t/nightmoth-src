//! Progress: one snapshot a day (taken while you're away, so it measures the idle PC),
//! used for the "before vs after" page and the weekly report.

use serde::Serialize;

use crate::config::{Snapshot, Stats};
use crate::state::{cfg, with_cfg};
use crate::win::now;

/// Measure the PC right now and store it.
pub fn snapshot() {
    let live = crate::stats::live();
    let health = crate::stats::health(true);
    let (free, _) = crate::win::system_drive_space();
    let boot_ms = crate::speed::boot_times().last().map(|b| b.ms).unwrap_or(0);
    let snap = Snapshot {
        t: now(),
        boot_ms,
        idle_cpu: live.cpu,
        idle_ram_mb: live.ram_used / 1_048_576,
        startup_apps: health.startup_enabled as u32,
        disk_free: free,
        junk_bytes: health.junk_bytes,
        apps: crate::apps::list().len() as u32,
        score: health.score,
    };
    with_cfg(|c| {
        c.history.push(snap);
        if c.history.len() > 400 {
            c.history.remove(1); // always keep the very first one: it's the "before"
        }
    });
}

#[derive(Serialize)]
pub struct Progress {
    pub first: Option<Snapshot>,
    pub latest: Option<Snapshot>,
    pub history: Vec<Snapshot>,
    pub week: Stats,
    pub last_week: Stats,
    pub week_start: i64,
    pub fixes: usize,
    pub removed: usize,
}

pub fn progress() -> Progress {
    let c = cfg();
    if c.history.is_empty() {
        // first visit: take the "before" picture now
        snapshot();
    }
    let c = cfg();
    Progress {
        first: c.history.first().cloned(),
        latest: c.history.last().cloned(),
        history: c.history.iter().rev().take(60).rev().cloned().collect(),
        week: c.stats.clone(),
        last_week: c.last_week.clone(),
        week_start: c.stats.week_start,
        fixes: c.fixes.len(),
        removed: c.removed_apps.len(),
    }
}
