//! "Run whenever it's needed": instead of fixed schedules, Autopilot reacts to what
//! the PC is doing — disk getting full, junk piling up, you stepping away, the charger
//! going in or out — and runs the right job at the right moment.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::changelog;
use crate::state::{cfg, core, with_cfg};
use crate::toast;
use crate::win::{self, now, Hive, RegVal};

const BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
static ON_BATTERY: AtomicBool = AtomicBool::new(false);
static WAS_ON_BATTERY: AtomicBool = AtomicBool::new(false);

pub fn on_battery() -> bool {
    ON_BATTERY.load(Ordering::Relaxed)
}

fn boosting() -> bool {
    core().boosting.load(Ordering::Relaxed)
}

/// Is the system drive running out of room? (< 10 % or < 15 GB free)
pub fn disk_low() -> bool {
    let (free, total) = win::system_drive_space();
    total > 0 && (free < total / 10 || free < 15_000_000_000)
}

fn junk_bytes() -> u64 {
    crate::speed::cleanup_scan().iter().filter(|t| t.id != "recycle").map(|t| t.bytes).sum()
}

fn cleanup(reason: &str) {
    let ids: Vec<String> = crate::speed::all_clean_ids().into_iter().filter(|i| i != "recycle").collect();
    let freed = crate::speed::cleanup_run(&ids, "Autopilot");
    with_cfg(|c| c.last_cleanup = now());
    if freed > 0 {
        toast::show("cleanup", "Cleanup done", &format!("Freed {} · {reason}", crate::fmt_bytes(freed)));
    }
}

// ------------------------------------------------------------------ laptop battery

/// On battery: drop from Ultimate/High Performance to Balanced (the app also pauses
/// the taskbar effect and slows the widgets). Back on the charger: put it back.
fn battery_check() {
    let (has_battery, on_batt, _) = win::power_source();
    ON_BATTERY.store(on_batt, Ordering::Relaxed);
    let c = cfg();
    if !has_battery || !c.battery_mode || boosting() {
        return;
    }
    let was = WAS_ON_BATTERY.swap(on_batt, Ordering::Relaxed);
    if on_batt && !was {
        let active = crate::speed::active_plan();
        if !active.eq_ignore_ascii_case(BALANCED) {
            with_cfg(|cf| cf.battery_prev_plan = active.clone());
            let _ = crate::speed::set_active_plan(BALANCED);
            toast::show("power", "Battery mode", "Balanced power · effects paused · widgets slowed");
        }
    } else if !on_batt && was {
        let prev = cfg().battery_prev_plan;
        if !prev.is_empty() {
            let _ = crate::speed::set_active_plan(&prev);
            with_cfg(|cf| cf.battery_prev_plan.clear());
            toast::show("power", "Plugged in", "Full power is back");
        }
    }
}

// ------------------------------------------------------------------ visual effects

const DESKTOP: &str = r"Control Panel\Desktop";
const METRICS: &str = r"Control Panel\Desktop\WindowMetrics";
const ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const VFX: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects";

/// A PC that struggles with Windows' animations: under 8 GB RAM or 4 threads or fewer.
pub fn weak_pc() -> bool {
    let sys = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything()).with_cpu(sysinfo::CpuRefreshKind::nothing()),
    );
    sys.total_memory() < 7_500_000_000 || sys.cpus().len() <= 4
}

/// Lighter visuals: no window animations, no fades — fonts stay smooth.
pub fn lighter_visuals(module: &str) -> Result<(), String> {
    let mut ch = vec![];
    ch.extend(changelog::reg(Hive::HKCU, METRICS, "MinAnimate", RegVal::Str("0".into()))?);
    ch.extend(changelog::reg(Hive::HKCU, ADVANCED, "TaskbarAnimations", RegVal::Dword(0))?);
    ch.extend(changelog::reg(Hive::HKCU, ADVANCED, "ListviewAlphaSelect", RegVal::Dword(0))?);
    ch.extend(changelog::reg(Hive::HKCU, VFX, "VisualFXSetting", RegVal::Dword(3))?);
    // "best performance" but keep smooth fonts and thumbnails
    ch.extend(changelog::reg(Hive::HKCU, DESKTOP, "UserPreferencesMask", RegVal::Bin(vec![0x90, 0x12, 0x03, 0x80, 0x10, 0x00, 0x00, 0x00]))?);
    ch.extend(changelog::reg(Hive::HKCU, DESKTOP, "FontSmoothing", RegVal::Str("2".into()))?);
    win::broadcast("WindowMetrics");
    changelog::record_many(module, "Lighter visual effects", "Animations off for a snappier PC", ch);
    Ok(())
}

pub fn visuals_light() -> bool {
    win::reg_str(Hive::HKCU, METRICS, "MinAnimate").as_deref() == Some("0")
}

fn visual_check() {
    let c = cfg();
    if c.visual_done {
        return;
    }
    with_cfg(|cf| cf.visual_done = true);
    if c.visual_auto && weak_pc() && !visuals_light() && lighter_visuals("Autopilot").is_ok() {
        toast::show("optimize", "Lighter visuals", "Your PC is on the lighter side — animations turned off");
    }
}

// ------------------------------------------------------------------ Windows Update

const WU_UX: &str = r"SOFTWARE\Microsoft\WindowsUpdate\UX\Settings";

/// Restarts only at night: active hours 08:00–02:00 (the longest Windows allows).
pub fn night_restarts(module: &str) -> Result<(), String> {
    let mut ch = vec![];
    ch.extend(changelog::reg(Hive::HKLM, WU_UX, "SmartActiveHoursState", RegVal::Dword(0))?);
    ch.extend(changelog::reg(Hive::HKLM, WU_UX, "ActiveHoursStart", RegVal::Dword(8))?);
    ch.extend(changelog::reg(Hive::HKLM, WU_UX, "ActiveHoursEnd", RegVal::Dword(2))?);
    changelog::record_many(module, "Update restarts only at night", "Active hours 08:00–02:00", ch);
    Ok(())
}

fn wu_check() {
    let c = cfg();
    if c.wu_control && win::reg_dword(Hive::HKLM, WU_UX, "ActiveHoursEnd") != Some(2) {
        let _ = night_restarts("Autopilot");
    }
}

// ------------------------------------------------------------------ the brain

/// Called every 30 s by the Autopilot loop.
pub fn tick(tick: u64) {
    let c = cfg();
    battery_check();
    let up = now() - core().started;
    if up < 5 * 60 {
        return;
    }
    if tick % 20 == 0 {
        visual_check();
        wu_check();
    }

    // tweaks left by other tools: soon after start, then twice a day
    if c.fix_other_tools && now() - c.last_fixer > 12 * 3600 && !boosting() {
        with_cfg(|cf| cf.last_fixer = now());
        crate::fixer::run_auto();
    }

    // cleanup: the disk is filling up, or junk has piled up
    if c.auto_cleanup && !boosting() && now() - c.last_cleanup > 3 * 3600 {
        if tick % 10 == 0 && disk_low() {
            cleanup("your drive was getting full");
        } else if now() - c.last_junk_check > 2 * 3600 && up > 15 * 60 {
            with_cfg(|cf| cf.last_junk_check = now());
            let junk = junk_bytes();
            let idle = win::idle_secs() > 5 * 60;
            if junk > 1_500_000_000 || (idle && junk > 400_000_000 && now() - c.last_cleanup > 3 * 86400) {
                cleanup(&format!("{} of junk had piled up", crate::fmt_bytes(junk)));
            }
        }
    }

    // heavier work waits until you're away (and not on a low battery)
    let (_, on_batt, pct) = win::power_source();
    let away = win::idle_secs() > 10 * 60 && !boosting() && (!on_batt || pct > 60);
    if !away {
        return;
    }
    let c = cfg();
    if now() - c.last_snapshot > 20 * 3600 {
        with_cfg(|cf| cf.last_snapshot = now());
        crate::history::snapshot();
    }
    if c.deep_clean && (now() - c.last_deep_clean > 7 * 86400 || (disk_low() && now() - c.last_deep_clean > 86400)) {
        with_cfg(|cf| cf.last_deep_clean = now());
        crate::deepclean::run_auto();
    }
    if c.auto_debloat && now() - c.last_debloat > 86400 {
        with_cfg(|cf| cf.last_debloat = now());
        let n = crate::speed::debloat_auto();
        if n > 0 {
            toast::show("optimize", "Junk apps removed", &format!("{n} preinstalled apps nobody needs"));
        }
    }
    if c.auto_updates && !on_batt && now() - c.last_update_check > 86400 {
        with_cfg(|cf| cf.last_update_check = now());
        crate::updates::run_auto();
    }
    if c.dns_auto && now() - c.dns_last > 14 * 86400 {
        with_cfg(|cf| cf.dns_last = now());
        let _ = crate::net::dns_auto();
    }
}
