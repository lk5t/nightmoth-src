//! Nightmoth — wallpaper-driven desktop glow-up + autopilot optimizer for Windows 11.

mod apps;
mod autopilot;
mod changelog;
mod commands;
mod config;
mod gallery;
mod games;
mod log;
mod look;
mod speed;
mod state;
mod stats;
mod toast;
mod win;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use std::time::Instant;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

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

/// Windows are created here (not in tauri.conf.json) so startup is logged step by step
/// and the main window appears before anything else is set up.
fn create_main(app: &tauri::App) -> tauri::Result<()> {
    let t = Instant::now();
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Nightmoth")
        .inner_size(1180.0, 760.0)
        .min_inner_size(980.0, 640.0)
        .decorations(false)
        .center()
        .visible(false)
        .build()?;
    log::log(&format!("main window created in {} ms", t.elapsed().as_millis()));
    Ok(())
}

fn create_toast(app: &AppHandle) -> tauri::Result<()> {
    let t = Instant::now();
    WebviewWindowBuilder::new(app, "toast", WebviewUrl::App("toast.html".into()))
        .title("Nightmoth")
        .inner_size(380.0, 104.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .shadow(false)
        .visible(false)
        .build()?;
    log::log(&format!("toast window created in {} ms", t.elapsed().as_millis()));
    Ok(())
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
    log::install_panic_hook();
    log::log(&format!("starting Nightmoth {} args={:?}", env!("CARGO_PKG_VERSION"), std::env::args().collect::<Vec<_>>()));
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            log::log("second launch: showing the existing window");
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            log::log("setup: main window");
            create_main(app)?;
            let tray_only = std::env::args().any(|a| a == "--tray");
            let first = !crate::config::Config::load().first_run_done;
            if !tray_only || first {
                show_main(app.handle());
            }
            log::log("setup: state");
            state::init(app.handle().clone());
            log::log("setup: toasts");
            toast::start();
            log::log("setup: tray");
            if let Err(e) = build_tray(app) {
                // the app still works without a tray icon
                log::log(&format!("tray failed: {e}"));
            }

            if first {
                std::thread::spawn(|| {
                    if state::cfg().autostart {
                        let _ = set_autostart(true);
                    }
                    state::with_cfg(|c| c.first_run_done = true);
                });
            }
            // the pop-up window is created a moment later so it never delays the main window
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(800));
                if let Err(e) = create_toast(&handle) {
                    log::log(&format!("toast window failed: {e}"));
                }
            });
            log::log("setup: autopilot");
            autopilot::run();
            log::log("setup: done");
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
            commands::ui_log,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        let msg = format!("Nightmoth couldn't start: {e}");
        log::log(&msg);
        log::error_box(&format!("{msg}\n\nIf this mentions WebView2, install it from https://go.microsoft.com/fwlink/p/?LinkId=2124703"));
    }
}
