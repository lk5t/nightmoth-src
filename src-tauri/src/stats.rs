//! Live system stats and the PC health score.

use serde::Serialize;
use std::sync::Mutex;
use sysinfo::{Disks, ProcessesToUpdate};

use crate::state::{cfg, core, lock};

#[derive(Serialize, Clone, Default)]
pub struct Live {
    pub cpu: f32,
    pub ram_used: u64,
    pub ram_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    pub uptime: u64,
    pub processes: usize,
    pub top: Vec<(String, f32, u64)>,
}

pub fn live() -> Live {
    let mut sys = lock(&core().sys);
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let ncpu = sys.cpus().len().max(1) as f32;
    let mut top: Vec<(String, f32, u64)> = sys
        .processes()
        .values()
        .map(|p| (p.name().to_string_lossy().to_string(), p.cpu_usage() / ncpu, p.memory() / 1024 / 1024))
        .filter(|p| p.0 != "System Idle Process")
        .collect();
    top.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    top.truncate(5);
    let (disk_used, disk_total) = system_disk();
    Live {
        cpu: sys.global_cpu_usage(),
        ram_used: sys.used_memory(),
        ram_total: sys.total_memory(),
        disk_used,
        disk_total,
        uptime: sysinfo::System::uptime(),
        processes: sys.processes().len(),
        top,
    }
}

fn system_disk() -> (u64, u64) {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_lowercase();
    let disks = Disks::new_with_refreshed_list();
    for d in disks.list() {
        let mp = d.mount_point().to_string_lossy().to_lowercase();
        if mp.starts_with(&drive) {
            return (d.total_space() - d.available_space(), d.total_space());
        }
    }
    (0, 0)
}

#[derive(Serialize, Clone, Default)]
pub struct Health {
    pub score: u32,
    pub issues: Vec<String>,
    pub startup_enabled: usize,
    pub junk_bytes: u64,
    pub unused_apps: usize,
    pub privacy_off: usize,
    pub computed: i64,
}

static HEALTH: Mutex<Option<Health>> = Mutex::new(None);

pub fn health(refresh: bool) -> Health {
    if !refresh {
        if let Some(h) = lock(&HEALTH).clone() {
            if crate::win::now() - h.computed < 600 {
                return h;
            }
        }
    }
    let mut score: i32 = 100;
    let mut issues = vec![];

    let startup_enabled = crate::speed::startup_list(false).iter().filter(|s| s.enabled).count();
    if startup_enabled > 6 {
        let p = ((startup_enabled - 6) as i32 * 2).min(20);
        score -= p;
        issues.push(format!("{startup_enabled} apps start with Windows"));
    }
    let junk: u64 = crate::speed::cleanup_scan().iter().map(|t| t.bytes).sum();
    let gb = junk as f64 / 1e9;
    if gb > 1.0 {
        score -= ((gb - 1.0) * 3.0).min(15.0) as i32;
        issues.push(format!("{} of junk files", crate::fmt_bytes(junk)));
    }
    let privacy_off = crate::speed::privacy_list().iter().filter(|p| !p.on).count();
    if privacy_off > 0 {
        score -= (privacy_off as i32 * 2).min(12);
        issues.push(format!("{privacy_off} ad/tracking settings still on"));
    }
    let unused = crate::apps::list().iter().filter(|a| a.candidate).count();
    if unused > 0 {
        score -= (unused as i32 * 2).min(15);
        issues.push(format!("{unused} apps unused for {}+ days", cfg().unused_days));
    }
    let (used, total) = system_disk();
    if total > 0 && (total - used) as f64 / (total as f64) < 0.12 {
        score -= 15;
        issues.push("System drive is almost full".into());
    }
    let l = live();
    if l.ram_total > 0 && l.ram_used as f64 / l.ram_total as f64 > 0.85 {
        score -= 8;
        issues.push("RAM is nearly full".into());
    }
    let h = Health {
        score: score.clamp(0, 100) as u32,
        issues,
        startup_enabled,
        junk_bytes: junk,
        unused_apps: unused,
        privacy_off,
        computed: crate::win::now(),
    };
    *lock(&HEALTH) = Some(h.clone());
    h
}
