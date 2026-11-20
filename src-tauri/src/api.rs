//! Every command the app can ask the engine to run. Nothing outside this list is accepted.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::config::Config;
use crate::state::{cfg, core, lock, with_cfg};
use crate::{apps, changelog, gallery, games, look, speed, stats, toast, win};

type R = Result<Value, String>;

fn arg<T: DeserializeOwned>(a: &Value, k: &str) -> Result<T, String> {
    serde_json::from_value(a.get(k).cloned().unwrap_or(Value::Null)).map_err(|e| format!("bad argument {k}: {e}"))
}

fn ok<T: Serialize>(v: T) -> R {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct Overview {
    live: stats::Live,
    stats: crate::config::Stats,
    last_week: crate::config::Stats,
    wallpaper: String,
    wallpaper_thumb: Option<String>,
    accent: Option<String>,
    style: Option<String>,
    boosting: Option<String>,
    autopilot: bool,
    version: String,
}

#[derive(Serialize)]
struct LookState {
    wallpaper: String,
    thumb: Option<String>,
    analysis: Option<look::Analysis>,
    accent: Option<String>,
    desktop: look::DesktopState,
    config: Config,
}

#[derive(Serialize)]
struct Preview {
    thumb: String,
    analysis: look::Analysis,
}

#[derive(Serialize)]
struct AppsView {
    apps: Vec<apps::InstalledApp>,
    removed: Vec<crate::config::RemovedApp>,
    threshold: u32,
}

fn current_wall(c: &Config) -> String {
    if c.current_wallpaper.is_empty() {
        win::current_wallpaper()
    } else {
        c.current_wallpaper.clone()
    }
}

pub fn dispatch(cmd: &str, a: &Value) -> R {
    match cmd {
        "ping" => ok("pong"),
        "shutdown" => {
            games::end_boost(false);
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(300));
                std::process::exit(0);
            });
            ok(true)
        }

        // ---------------------------------------------------------- config
        "get_config" => ok(cfg()),
        "set_config" => {
            let patch: Value = a.get("patch").cloned().unwrap_or(json!({}));
            let mut cur = serde_json::to_value(cfg()).map_err(|e| e.to_string())?;
            if let (Some(obj), Some(p)) = (cur.as_object_mut(), patch.as_object()) {
                for (k, v) in p {
                    obj.insert(k.clone(), v.clone());
                }
            }
            let next: Config = serde_json::from_value(cur).map_err(|e| e.to_string())?;
            with_cfg(|c| *c = next.clone());
            ok(next)
        }
        "toggle_autopilot" => {
            let on = with_cfg(|c| {
                c.autopilot = !c.autopilot;
                c.autopilot
            });
            toast::show("settings", if on { "Autopilot on" } else { "Autopilot paused" }, "Change it any time from the tray");
            ok(on)
        }

        // ---------------------------------------------------------- home
        "overview" => {
            let c = cfg();
            let wall = current_wall(&c);
            let thumb = if wall.is_empty() { None } else { look::thumb(&wall, 640).ok() };
            let lk = c.looks.get(&wall);
            ok(Overview {
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
        }
        "live_stats" => ok(stats::live()),
        "health" => ok(stats::health(arg::<bool>(a, "refresh").unwrap_or(false))),

        // ---------------------------------------------------------- look
        "look_state" => {
            let c = cfg();
            let wall = current_wall(&c);
            let (thumb, analysis) = if wall.is_empty() { (None, None) } else { (look::thumb(&wall, 900).ok(), look::analyze(&wall).ok()) };
            ok(LookState {
                accent: c.looks.get(&wall).map(|l| l.accent.clone()),
                wallpaper: wall,
                thumb,
                analysis,
                desktop: look::desktop_state(),
                config: c,
            })
        }
        "look_apply" => ok(look::apply(&arg::<String>(a, "path")?, true)?),
        "look_preview" => {
            let path: String = arg(a, "path")?;
            ok(Preview { thumb: look::thumb(&path, 900)?, analysis: look::analyze(&path)? })
        }
        "look_shuffle" => ok(look::shuffle()?),
        "look_lock_accent" => {
            let hex: Option<String> = arg(a, "hex")?;
            with_cfg(|c| c.lock_accent = hex);
            let wall = cfg().current_wallpaper;
            if !wall.is_empty() {
                look::apply(&wall, true)?;
            }
            ok(true)
        }
        "desktop_set" => {
            let key: String = arg(a, "key")?;
            let value = a.get("value").cloned().unwrap_or(Value::Null);
            look::desktop_set(&key, &value)?;
            std::thread::sleep(std::time::Duration::from_millis(300));
            ok(look::desktop_state())
        }
        "preset_apply" => ok(look::preset(&arg::<String>(a, "id")?)?),
        "gallery_list" => ok(gallery::list()?),
        "slideshow_set" => {
            let on: bool = arg(a, "on")?;
            let folder: String = arg(a, "folder")?;
            let minutes: u32 = arg(a, "minutes")?;
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
            ok(n)
        }
        "slideshow_next" => {
            look::slideshow_next();
            ok(true)
        }

        // ---------------------------------------------------------- speed
        "startup_list" => ok(speed::startup_list(true)),
        "startup_set" => {
            let id: String = arg(a, "id")?;
            let enabled: bool = arg(a, "enabled")?;
            speed::startup_set(&id, enabled, "Speed")?;
            with_cfg(|c| {
                if enabled && !c.startup_allow.contains(&id) {
                    c.startup_allow.push(id.clone());
                }
                if !c.known_startup.contains(&id) {
                    c.known_startup.push(id.clone());
                }
            });
            ok(true)
        }
        "cleanup_scan" => ok(speed::cleanup_scan()),
        "cleanup_run" => {
            let ids: Vec<String> = arg(a, "ids")?;
            let freed = speed::cleanup_run(&ids, "Speed");
            toast::show("cleanup", "Cleanup done", &format!("Freed {}", crate::fmt_bytes(freed)));
            ok(freed)
        }
        "debloat_list" => ok(speed::debloat_list()),
        "debloat_remove" => {
            let ids: Vec<String> = arg(a, "ids")?;
            let _ = speed::restore_point(false);
            let (n, errs) = speed::debloat_remove(&ids);
            if n > 0 {
                toast::show("optimize", "Bloat removed", &format!("{n} preinstalled apps removed"));
            }
            if n == 0 && !errs.is_empty() {
                return Err(errs.join("\n"));
            }
            ok(n)
        }
        "services_list" => ok(speed::services_list()),
        "service_set" => {
            let _ = speed::restore_point(false);
            speed::service_set(&arg::<String>(a, "name")?, arg(a, "disabled")?, "Speed")?;
            ok(true)
        }
        "privacy_list" => ok(speed::privacy_list()),
        "privacy_set" => {
            speed::privacy_set(&arg::<String>(a, "id")?, arg(a, "on")?, "Speed")?;
            ok(true)
        }
        "power_state" => ok(speed::power_state()),
        "power_set" => {
            let guid: String = arg(a, "guid")?;
            let g = if guid == "ultimate" { speed::ultimate_guid()? } else { guid };
            speed::set_plan_logged(&g, "Speed")?;
            ok(speed::power_state())
        }
        "hags_set" => {
            speed::hags_set(arg(a, "on")?, "Speed")?;
            ok(true)
        }
        "game_mode_set" => {
            speed::game_mode_set(arg(a, "on")?, "Speed")?;
            ok(true)
        }
        "boot_times" => ok(speed::boot_times()),
        "optimize_now" => ok(speed::optimize_now()),

        // ---------------------------------------------------------- apps
        "apps_list" => {
            let c = cfg();
            ok(AppsView { apps: apps::list(), removed: c.removed_apps.clone(), threshold: c.unused_days })
        }
        "app_keep" => {
            let name: String = arg(a, "name")?;
            let keep: bool = arg(a, "keep")?;
            with_cfg(|c| {
                c.keep_list.retain(|k| k != &name);
                if keep {
                    c.keep_list.push(name);
                }
            });
            ok(true)
        }
        "app_remove" => ok(apps::uninstall_by_key(&arg::<String>(a, "key")?)?),
        "app_reinstall" => {
            apps::reinstall(&arg::<String>(a, "name")?)?;
            ok(true)
        }

        // ---------------------------------------------------------- games
        "games_list" => ok(games::list()),
        "game_boost_set" => {
            games::set_boost(&arg::<String>(a, "exe")?, arg(a, "on")?);
            ok(true)
        }
        "processes" => ok(games::processes()),

        // ---------------------------------------------------------- safety
        "changes_list" => {
            let log = lock(&core().log);
            ok(log.entries.iter().rev().take(400).cloned().collect::<Vec<_>>())
        }
        "change_undo" => ok(changelog::undo(arg(a, "id")?)?),
        "undo_all" => {
            games::end_boost(false);
            let (n, errs) = changelog::undo_all();
            toast::show("settings", "Everything reverted", &format!("{n} changes undone"));
            if n == 0 && !errs.is_empty() {
                return Err(errs.join("\n"));
            }
            ok(n)
        }
        "restore_point" => {
            speed::restore_point(true)?;
            ok(true)
        }

        // ---------------------------------------------------------- pop-ups
        "toast_take" => ok(toast::take()),
        "toast_shown" => {
            toast::mark_shown(&arg::<String>(a, "id")?);
            ok(true)
        }
        "toast_action" => {
            toast::action(&arg::<String>(a, "id")?);
            ok(true)
        }
        "toast_test" => {
            toast::show_action("test", "Nightmoth pop-up", "This is how Autopilot tells you what it did", "Nice", Box::new(|| {}));
            ok(true)
        }

        _ => Err(format!("Unknown command: {cmd}")),
    }
}
