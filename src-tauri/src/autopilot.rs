//! Background loop: slideshow, startup watcher, hog watcher, scheduled cleanup,
//! unused-app cleaner, weekly report.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Duration;
use sysinfo::ProcessesToUpdate;

use crate::state::{cfg, core, lock, with_cfg};
use crate::toast;
use crate::win::{self, now, Priority};

const IGNORE_HOGS: &[&str] = &[
    "system", "system idle process", "registry", "memory compression", "msmpeng.exe", "dwm.exe", "explorer.exe",
    "svchost.exe", "csrss.exe", "wininit.exe", "services.exe", "lsass.exe", "nightmoth.exe", "searchindexer.exe",
    "tiworker.exe", "trustedinstaller.exe", "mpdefendercoreservice.exe", "audiodg.exe", "ntoskrnl.exe",
];

pub fn run() {
    std::thread::spawn(|| crate::games::watcher());
    std::thread::spawn(|| {
        let mut tick: u64 = 0;
        // pid -> (first time over the limit, already warned)
        let mut hogs: HashMap<u32, (i64, bool)> = HashMap::new();
        loop {
            std::thread::sleep(Duration::from_secs(30));
            tick += 1;
            let c = cfg();
            if !c.autopilot {
                continue;
            }
            crate::look::slideshow_tick();

            if c.startup_watcher && tick % 10 == 1 {
                crate::speed::startup_watch();
            }
            if c.hog_watcher && tick % 2 == 0 && !core().boosting.load(Ordering::Relaxed) {
                hog_watch(&mut hogs);
            }
            if c.auto_cleanup && now() - c.last_cleanup > c.cleanup_every_days.max(1) as i64 * 86400 && now() - core().started > 15 * 60 && !core().boosting.load(Ordering::Relaxed) {
                let freed = crate::speed::cleanup_run(&crate::speed::all_clean_ids().into_iter().filter(|i| i != "recycle").collect::<Vec<_>>(), "Autopilot");
                toast::show("cleanup", "Cleanup done", &format!("Freed {} of junk", crate::fmt_bytes(freed)));
            }
            if tick % 20 == 5 {
                crate::apps::unused_tick();
            }
            if now() - c.last_report > 7 * 86400 {
                weekly_report();
            }
        }
    });
}

fn hog_watch(hogs: &mut HashMap<u32, (i64, bool)>) {
    let c = cfg();
    let fg = win::foreground_pid();
    let over: Vec<(u32, String, f32, u64)> = {
        let mut sys = lock(&core().sys);
        sys.refresh_cpu_usage();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let ncpu = sys.cpus().len().max(1) as f32;
        sys.processes()
            .values()
            .filter_map(|p| {
                let name = p.name().to_string_lossy().to_string();
                if IGNORE_HOGS.contains(&name.to_lowercase().as_str()) {
                    return None;
                }
                let pid = p.pid().as_u32();
                if pid == fg {
                    return None;
                }
                let cpu = p.cpu_usage() / ncpu;
                let mb = p.memory() / 1024 / 1024;
                if cpu > c.hog_cpu || mb > c.hog_ram_mb {
                    Some((pid, name, cpu, mb))
                } else {
                    None
                }
            })
            .collect()
    };
    let t = now();
    let pids: Vec<u32> = over.iter().map(|o| o.0).collect();
    hogs.retain(|pid, _| pids.contains(pid));
    for (pid, name, cpu, mb) in over {
        let entry = hogs.entry(pid).or_insert((t, false));
        if !entry.1 && t - entry.0 > 5 * 60 {
            entry.1 = true;
            let what = if cpu > c.hog_cpu { format!("{:.0}% CPU", cpu) } else { format!("{} MB RAM", mb) };
            if c.hog_auto_lower {
                win::set_priority(pid, Priority::BelowNormal);
                toast::show("hog", &format!("{name} slowed down"), &format!("It was using {what} in the background"));
            } else {
                toast::show_action(
                    "hog",
                    &format!("{name} is hogging your PC"),
                    &format!("{what} in the background for 5+ min"),
                    "Slow it down",
                    Box::new(move || {
                        win::set_priority(pid, Priority::BelowNormal);
                    }),
                );
            }
        }
    }
}

fn weekly_report() {
    let c = cfg();
    let s = c.stats.clone();
    let body = format!(
        "Freed {} · {} apps removed · {} games boosted · {} startup apps blocked",
        crate::fmt_bytes(s.freed_bytes),
        s.apps_removed,
        s.games_boosted,
        s.startup_blocked
    );
    toast::show("report", "Your week with Nightmoth", &body);
    with_cfg(|cf| {
        cf.last_week = cf.stats.clone();
        cf.stats = crate::config::Stats { week_start: now(), ..Default::default() };
        cf.last_report = now();
    });
}
