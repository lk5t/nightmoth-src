//! Commands called from the UI. Slow work runs on a blocking thread.

use serde::Serialize;
use serde_json::Value;

use crate::config::Config;
use crate::state::{cfg, core, with_cfg};
use crate::{apps, changelog, gallery, games, look, speed, stats, toast, win};

type R<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> R<T> + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

// ------------------------------------------------------------------ config

#[tauri::command]
pub fn get_config() -> Config {
    cfg()
}

/// Merge a partial config object into the saved config.
#[tauri::command]
pub fn set_config(patch: Value) -> R<Config> {
    let mut cur = serde_json::to_value(cfg()).map_err(|e| e.to_string())?;
    if let (Some(obj), Some(p)) = (cur.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            obj.insert(k.clone(), v.clone());
        }
    }
    let next: Config = serde_json::from_value(cur).map_err(|e| e.to_string())?;
    with_cfg(|c| *c = next.clone());
    Ok(next)
}

// ------------------------------------------------------------------ home

#[derive(Serialize)]
pub struct Overview {
    pub live: stats::Live,
    pub stats: crate::config::Stats,
    pub last_week: crate::config::Stats,
    pub wallpaper: String,
    pub wallpaper_thumb: Option<String>,
    pub accent: Option<String>,
    pub style: Option<String>,
    pub boosting: Option<String>,
    pub autopilot: bool,
    pub version: String,
}

#[tauri::command]
pub async fn overview() -> R<Overview> {
    blocking(|| {
        let c = cfg();
        let wall = if c.current_wallpaper.is_empty() { win::current_wallpaper() } else { c.current_wallpaper.clone() };
        let thumb = if wall.is_empty() { None } else { look::thumb(&wall, 640).ok() };
        let lk = c.looks.get(&wall);
        Ok(Overview {
            live: stats::live(),
            stats: c.stats.clone(),
            last_week: c.last_week.clone(),
            accent: lk.map(|l| l.accent.clone()),
            style: lk.map(|l| l.style.clone()),
            wallpaper: wall,
            wallpaper_thumb: thumb,
            boosting: c.pending_boost.as_ref().map(|b| b.game.clone()),
            autopilot: c.autopilot,
            version: env!("CARGO_PKG_VERSION").into(),
        })
    })
    .await
}

#[tauri::command]
pub async fn live_stats() -> R<stats::Live> {
    blocking(|| Ok(stats::live())).await
}

#[tauri::command]
pub async fn health(refresh: bool) -> R<stats::Health> {
    blocking(move || Ok(stats::health(refresh))).await
}

// ------------------------------------------------------------------ look

#[derive(Serialize)]
pub struct LookState {
    pub wallpaper: String,
    pub thumb: Option<String>,
    pub analysis: Option<look::Analysis>,
    pub accent: Option<String>,
    pub desktop: look::DesktopState,
    pub config: Config,
}

#[tauri::command]
pub async fn look_state() -> R<LookState> {
    blocking(|| {
        let c = cfg();
        let wall = if c.current_wallpaper.is_empty() { win::current_wallpaper() } else { c.current_wallpaper.clone() };
        let (thumb, analysis) = if wall.is_empty() { (None, None) } else { (look::thumb(&wall, 900).ok(), look::analyze(&wall).ok()) };
        Ok(LookState {
            accent: c.looks.get(&wall).map(|l| l.accent.clone()),
            wallpaper: wall,
            thumb,
            analysis,
            desktop: look::desktop_state(),
            config: c,
        })
    })
    .await
}

#[tauri::command]
pub async fn look_apply(path: String) -> R<look::Applied> {
    blocking(move || look::apply(&path, true)).await
}

#[derive(Serialize)]
pub struct Preview {
    pub thumb: String,
    pub analysis: look::Analysis,
}

#[tauri::command]
pub async fn look_preview(path: String) -> R<Preview> {
    blocking(move || Ok(Preview { thumb: look::thumb(&path, 900)?, analysis: look::analyze(&path)? })).await
}

#[tauri::command]
pub async fn look_shuffle() -> R<look::Applied> {
    blocking(look::shuffle).await
}

#[tauri::command]
pub async fn look_lock_accent(hex: Option<String>) -> R<()> {
    blocking(move || {
        with_cfg(|c| c.lock_accent = hex);
        let wall = cfg().current_wallpaper;
        if !wall.is_empty() {
            look::apply(&wall, true)?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn desktop_set(key: String, value: Value) -> R<look::DesktopState> {
    blocking(move || {
        look::desktop_set(&key, &value)?;
        std::thread::sleep(std::time::Duration::from_millis(300));
        Ok(look::desktop_state())
    })
    .await
}

#[tauri::command]
pub async fn preset_apply(id: String) -> R<String> {
    blocking(move || look::preset(&id)).await
}

#[tauri::command]
pub async fn gallery_list() -> R<Vec<gallery::GalleryItem>> {
    blocking(gallery::list).await
}

#[tauri::command]
pub async fn slideshow_set(on: bool, folder: String, minutes: u32) -> R<usize> {
    blocking(move || {
        let n = look::folder_images(&folder).len();
        if on && n == 0 {
            return Err("That folder has no images".into());
        }
        with_cfg(|c| {
            c.slideshow_on = on;
            c.slideshow_folder = folder.clone();
            c.slideshow_minutes = minutes.max(1);
        });
        if on {
            look::slideshow_next();
        }
        Ok(n)
    })
    .await
}

#[tauri::command]
pub async fn slideshow_next() -> R<()> {
    blocking(|| {
        look::slideshow_next();
        Ok(())
    })
    .await
}

// ------------------------------------------------------------------ speed

#[tauri::command]
pub async fn startup_list() -> R<Vec<speed::StartupItem>> {
    blocking(|| Ok(speed::startup_list(true))).await
}

#[tauri::command]
pub async fn startup_set(id: String, enabled: bool) -> R<()> {
    blocking(move || {
        speed::startup_set(&id, enabled, "Speed")?;
        // the user decided: don't let Autopilot fight it
        with_cfg(|c| {
            if enabled && !c.startup_allow.contains(&id) {
                c.startup_allow.push(id.clone());
            }
            if !c.known_startup.contains(&id) {
                c.known_startup.push(id.clone());
            }
        });
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn cleanup_scan() -> R<Vec<speed::CleanTarget>> {
    blocking(|| Ok(speed::cleanup_scan())).await
}

#[tauri::command]
pub async fn cleanup_run(ids: Vec<String>) -> R<u64> {
    blocking(move || {
        let freed = speed::cleanup_run(&ids, "Speed");
        toast::show("cleanup", "Cleanup done", &format!("Freed {}", crate::fmt_bytes(freed)));
        Ok(freed)
    })
    .await
}

#[tauri::command]
pub async fn debloat_list() -> R<Vec<speed::BloatApp>> {
    blocking(|| Ok(speed::debloat_list())).await
}

#[tauri::command]
pub async fn debloat_remove(ids: Vec<String>) -> R<u32> {
    blocking(move || {
        let _ = speed::restore_point(false);
        let (n, errs) = speed::debloat_remove(&ids);
        if n > 0 {
            toast::show("optimize", "Bloat removed", &format!("{n} preinstalled apps removed"));
        }
        if n == 0 && !errs.is_empty() {
            return Err(errs.join("\n"));
        }
        Ok(n)
    })
    .await
}

#[tauri::command]
pub async fn services_list() -> R<Vec<speed::ServiceItem>> {
    blocking(|| Ok(speed::services_list())).await
}

#[tauri::command]
pub async fn service_set(name: String, disabled: bool) -> R<()> {
    blocking(move || {
        let _ = speed::restore_point(false);
        speed::service_set(&name, disabled, "Speed")
    })
    .await
}

#[tauri::command]
pub async fn privacy_list() -> R<Vec<speed::PrivacyItem>> {
    blocking(|| Ok(speed::privacy_list())).await
}

#[tauri::command]
pub async fn privacy_set(id: String, on: bool) -> R<()> {
    blocking(move || speed::privacy_set(&id, on, "Speed")).await
}

#[tauri::command]
pub async fn power_state() -> R<speed::PowerState> {
    blocking(|| Ok(speed::power_state())).await
}

/// guid = "ultimate" creates/uses the Ultimate Performance plan.
#[tauri::command]
pub async fn power_set(guid: String) -> R<speed::PowerState> {
    blocking(move || {
        let g = if guid == "ultimate" { speed::ultimate_guid()? } else { guid };
        speed::set_plan_logged(&g, "Speed")?;
        Ok(speed::power_state())
    })
    .await
}

#[tauri::command]
pub async fn hags_set(on: bool) -> R<()> {
    blocking(move || speed::hags_set(on, "Speed")).await
}

#[tauri::command]
pub async fn game_mode_set(on: bool) -> R<()> {
    blocking(move || speed::game_mode_set(on, "Speed")).await
}

#[tauri::command]
pub async fn boot_times() -> R<Vec<speed::Boot>> {
    blocking(|| Ok(speed::boot_times())).await
}

#[tauri::command]
pub async fn optimize_now() -> R<speed::OptimizeReport> {
    blocking(|| Ok(speed::optimize_now())).await
}

// ------------------------------------------------------------------ apps

#[derive(Serialize)]
pub struct AppsView {
    pub apps: Vec<apps::InstalledApp>,
    pub removed: Vec<crate::config::RemovedApp>,
    pub threshold: u32,
}

#[tauri::command]
pub async fn apps_list() -> R<AppsView> {
    blocking(|| {
        let c = cfg();
        Ok(AppsView { apps: apps::list(), removed: c.removed_apps.clone(), threshold: c.unused_days })
    })
    .await
}

#[tauri::command]
pub fn app_keep(name: String, keep: bool) {
    with_cfg(|c| {
        c.keep_list.retain(|k| k != &name);
        if keep {
            c.keep_list.push(name);
        }
    });
}

#[tauri::command]
pub async fn app_remove(key: String) -> R<String> {
    blocking(move || apps::uninstall_by_key(&key)).await
}

#[tauri::command]
pub async fn app_reinstall(name: String) -> R<()> {
    blocking(move || apps::reinstall(&name)).await
}

// ------------------------------------------------------------------ games

#[tauri::command]
pub fn games_list() -> Vec<games::GameInfo> {
    games::list()
}

#[tauri::command]
pub fn game_boost_set(exe: String, on: bool) {
    games::set_boost(&exe, on);
}

#[tauri::command]
pub async fn processes() -> R<Vec<games::Proc>> {
    blocking(|| Ok(games::processes())).await
}

// ------------------------------------------------------------------ safety

#[tauri::command]
pub fn changes_list() -> Vec<changelog::Entry> {
    let log = crate::state::lock(&core().log);
    log.entries.iter().rev().take(400).cloned().collect()
}

#[tauri::command]
pub async fn change_undo(id: u64) -> R<String> {
    blocking(move || changelog::undo(id)).await
}

#[tauri::command]
pub async fn undo_all() -> R<u32> {
    blocking(|| {
        crate::games::end_boost(false);
        let (n, errs) = changelog::undo_all();
        toast::show("settings", "Everything reverted", &format!("{n} changes undone"));
        if n == 0 && !errs.is_empty() {
            return Err(errs.join("\n"));
        }
        Ok(n)
    })
    .await
}

#[tauri::command]
pub async fn restore_point() -> R<()> {
    blocking(|| speed::restore_point(true)).await
}

#[tauri::command]
pub async fn autostart_set(on: bool) -> R<()> {
    blocking(move || crate::set_autostart(on)).await
}

// ------------------------------------------------------------------ toasts

#[tauri::command]
pub fn toast_done() {
    toast::done();
}

#[tauri::command]
pub fn toast_action(id: String) {
    toast::action(&id);
}

#[tauri::command]
pub fn toast_test() {
    toast::show_action("test", "Nightmoth pop-up", "This is how Autopilot tells you what it did", "Nice", Box::new(|| {}));
}

#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    crate::games::end_boost(false);
    app.exit(0);
}

/// Lets the UI write to the log (startup + JS errors).
#[tauri::command]
pub fn ui_log(msg: String) {
    if msg == "toast window ready" {
        toast::READY.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    crate::log::log(&format!("ui: {msg}"));
}
