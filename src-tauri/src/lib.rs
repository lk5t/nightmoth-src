//! Nightmoth — wallpaper-driven desktop glow-up + autopilot optimizer for Windows 11.

mod apps;
mod autopilot;
mod changelog;
mod commands;
mod config;
mod gallery;
mod games;
mod look;
mod speed;
mod state;
mod stats;
mod toast;
mod win;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub fn fmt_bytes(b: u64) -> String {
    let b = b as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{} B", b as u64)
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Register (or remove) the elevated "start at logon" task.
pub fn set_autostart(on: bool) -> Result<(), String> {
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = win::ps_quote(&exe.to_string_lossy());
        let script = format!(
            "$a = New-ScheduledTaskAction -Execute {exe} -Argument '--tray'; \
             $t = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME; $t.Delay = 'PT15S'; \
             $p = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Highest; \
             $s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew; \
             Register-ScheduledTask -TaskName 'Nightmoth' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null"
        );
        win::ps(&script)?;
    } else {
        let _ = win::ps("Unregister-ScheduledTask -TaskName 'Nightmoth' -Confirm:$false -ErrorAction SilentlyContinue");
    }
    state::with_cfg(|c| c.autostart = on);
    Ok(())
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Nightmoth", true, None::<&str>)?;
    let optimize = MenuItem::with_id(app, "optimize", "Optimize now", true, None::<&str>)?;
    let autopilot = CheckMenuItem::with_id(app, "autopilot", "Autopilot", true, state::cfg().autopilot, None::<&str>)?;
    let next = MenuItem::with_id(app, "next_wall", "Next wallpaper", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &optimize, &next, &autopilot, &sep, &quit])?;

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    let ap = autopilot.clone();
    TrayIconBuilder::with_id("nightmoth")
        .icon(icon)
        .tooltip("Nightmoth")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "optimize" => {
                std::thread::spawn(|| {
                    crate::speed::optimize_now();
                });
            }
            "next_wall" => {
                std::thread::spawn(|| crate::look::slideshow_next());
            }
            "autopilot" => {
                let on = state::with_cfg(|c| {
                    c.autopilot = !c.autopilot;
                    c.autopilot
                });
                let _ = ap.set_checked(on);
                toast::show("settings", if on { "Autopilot on" } else { "Autopilot paused" }, "Change it any time from the tray");
            }
            "quit" => {
                crate::games::end_boost(false);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            state::init(app.handle().clone());
            toast::start();
            build_tray(app)?;

            let first = !state::cfg().first_run_done;
            if first {
                std::thread::spawn(|| {
                    if state::cfg().autostart {
                        let _ = set_autostart(true);
                    }
                    state::with_cfg(|c| c.first_run_done = true);
                });
            }
            let tray_only = std::env::args().any(|a| a == "--tray");
            if !tray_only || first {
                show_main(app.handle());
            }
            autopilot::run();
            if tray_only {
                toast::show("settings", "Nightmoth is running", "Autopilot is keeping your PC fast");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::set_config,
            commands::overview,
            commands::live_stats,
            commands::health,
            commands::look_state,
            commands::look_apply,
            commands::look_preview,
            commands::look_shuffle,
            commands::look_lock_accent,
            commands::desktop_set,
            commands::preset_apply,
            commands::gallery_list,
            commands::slideshow_set,
            commands::slideshow_next,
            commands::startup_list,
            commands::startup_set,
            commands::cleanup_scan,
            commands::cleanup_run,
            commands::debloat_list,
            commands::debloat_remove,
            commands::services_list,
            commands::service_set,
            commands::privacy_list,
            commands::privacy_set,
            commands::power_state,
            commands::power_set,
            commands::hags_set,
            commands::game_mode_set,
            commands::boot_times,
            commands::optimize_now,
            commands::apps_list,
            commands::app_keep,
            commands::app_remove,
            commands::app_reinstall,
            commands::games_list,
            commands::game_boost_set,
            commands::processes,
            commands::changes_list,
            commands::change_undo,
            commands::undo_all,
            commands::restore_point,
            commands::autostart_set,
            commands::toast_done,
            commands::toast_action,
            commands::toast_test,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Nightmoth");
}
