//! Nightmoth — wallpaper-driven desktop glow-up + autopilot optimizer for Windows 11.
//!
//! One exe, three modes:
//! - `nightmoth.exe`                 the app (window, tray, pop-ups) — normal user rights
//! - `nightmoth.exe --engine`        the engine (all system changes + Autopilot) — admin
//! - `nightmoth.exe --setup-engine`  one-time, elevated: registers the engine's scheduled task

mod api;
mod apps;
mod autopilot;
mod changelog;
mod config;
mod gallery;
mod games;
mod ipc;
mod log;
mod look;
mod speed;
mod state;
mod stats;
mod toast;
mod ui_toast;
mod win;

use serde_json::{json, Value};
use std::time::Instant;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
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

/// Entry point: pick the mode from the command line.
pub fn main_entry() {
    log::install_panic_hook();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--engine") {
        log::set_role("engine");
        run_engine();
    } else if args.iter().any(|a| a == "--setup-engine") {
        log::set_role("setup");
        match ipc::setup_engine() {
            Ok(_) => log::log("engine task registered"),
            Err(e) => {
                log::log(&format!("engine task setup failed: {e}"));
                log::error_box(&format!("Nightmoth couldn't set up its background engine:\n\n{e}"));
            }
        }
    } else {
        log::set_role("app");
        run_app();
    }
}

// ================================================================== engine

fn run_engine() {
    log::log(&format!("engine {} starting", env!("CARGO_PKG_VERSION")));
    if ipc::engine_alive() {
        log::log("engine already running, exiting");
        return;
    }
    state::init();
    autopilot::run();
    ipc::serve(api::dispatch);
}

// ================================================================== app

/// Autostart for the app = HKCU Run key (no admin needed). The app starts the engine.
fn set_autostart(on: bool) -> Result<(), String> {
    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        win::reg_set(win::Hive::HKCU, RUN, "Nightmoth", &win::RegVal::Str(format!("\"{}\" --tray", exe.to_string_lossy())))
    } else {
        win::reg_delete(win::Hive::HKCU, RUN, "Nightmoth")
    }
}

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

fn engine_bg(cmd: &'static str) {
    std::thread::spawn(move || {
        if let Err(e) = ipc::call(cmd, json!({})) {
            ui_toast::local("Nightmoth", &e);
        }
    });
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Nightmoth", true, None::<&str>)?;
    let optimize = MenuItem::with_id(app, "optimize", "Optimize now", true, None::<&str>)?;
    let autopilot = CheckMenuItem::with_id(app, "autopilot", "Autopilot", true, true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next_wall", "Next wallpaper", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &optimize, &next, &autopilot, &sep, &quit])?;

    // show the real Autopilot state once the engine answers
    let ap_init = autopilot.clone();
    std::thread::spawn(move || {
        if let Ok(c) = ipc::call("get_config", json!({})) {
            let _ = ap_init.set_checked(c["autopilot"].as_bool().unwrap_or(true));
        }
    });

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    let ap = autopilot.clone();
    TrayIconBuilder::with_id("nightmoth")
        .icon(icon)
        .tooltip("Nightmoth")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "optimize" => engine_bg("optimize_now"),
            "next_wall" => engine_bg("slideshow_next"),
            "autopilot" => {
                let ap = ap.clone();
                std::thread::spawn(move || {
                    if let Ok(v) = ipc::call("toggle_autopilot", json!({})) {
                        let _ = ap.set_checked(v.as_bool().unwrap_or(true));
                    }
                });
            }
            "quit" => {
                let _ = ipc::call_quiet("shutdown", json!({}));
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

// ------------------------------------------------------------------ app commands

/// Forward a command to the engine.
#[tauri::command]
async fn engine(cmd: String, args: Option<Value>) -> Result<Value, String> {
    let args = args.unwrap_or_else(|| json!({}));
    tauri::async_runtime::spawn_blocking(move || ipc::call(&cmd, args))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn ui_log(msg: String) {
    if msg == "toast window ready" {
        ui_toast::READY.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    log::log(&format!("ui: {msg}"));
}

#[tauri::command]
fn toast_done() {
    ui_toast::done();
}

#[tauri::command]
fn toast_action(id: String) {
    std::thread::spawn(move || {
        let _ = ipc::call_quiet("toast_action", json!({ "id": id }));
    });
}

#[tauri::command]
fn autostart_get() -> bool {
    win::reg_str(win::Hive::HKCU, r"Software\Microsoft\Windows\CurrentVersion\Run", "Nightmoth").is_some()
}

#[tauri::command]
fn autostart_set(on: bool) -> Result<(), String> {
    set_autostart(on)
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    let _ = ipc::call_quiet("shutdown", json!({}));
    app.exit(0);
}

fn run_app() {
    log::log(&format!("app {} starting args={:?}", env!("CARGO_PKG_VERSION"), std::env::args().collect::<Vec<_>>()));
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            log::log("second launch: showing the existing window");
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            create_main(app)?;
            let tray_only = std::env::args().any(|a| a == "--tray");
            if !tray_only {
                show_main(app.handle());
            }
            if let Err(e) = build_tray(app) {
                log::log(&format!("tray failed: {e}"));
            }
            ui_toast::start(app.handle().clone());
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(600));
                if let Err(e) = create_toast(&handle) {
                    log::log(&format!("toast window failed: {e}"));
                }
            });
            // start the engine in the background and finish first-run setup
            std::thread::spawn(move || match ipc::ensure_engine() {
                Ok(_) => {
                    log::log("app: engine connected");
                    if let Ok(c) = ipc::call("get_config", json!({})) {
                        if !c["first_run_done"].as_bool().unwrap_or(false) {
                            if c["autostart"].as_bool().unwrap_or(true) {
                                let _ = set_autostart(true);
                            }
                            let _ = ipc::call("set_config", json!({ "patch": { "first_run_done": true } }));
                        }
                    }
                    if tray_only {
                        ui_toast::local("Nightmoth is running", "Autopilot is keeping your PC fast");
                    }
                }
                Err(e) => {
                    log::log(&format!("app: engine unavailable: {e}"));
                    ui_toast::local("Engine not running", &e);
                }
            });
            log::log("app: setup done");
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
            engine,
            ui_log,
            toast_done,
            toast_action,
            autostart_get,
            autostart_set,
            quit_app,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        let msg = format!("Nightmoth couldn't start: {e}");
        log::log(&msg);
        log::error_box(&format!("{msg}\n\nIf this mentions WebView2, install it from https://go.microsoft.com/fwlink/p/?LinkId=2124703"));
    }
}
