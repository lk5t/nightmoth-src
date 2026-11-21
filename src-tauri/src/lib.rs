//! Nightmoth — wallpaper-driven desktop glow-up + autopilot optimizer for Windows 11.
//!
//! One exe, three modes:
//! - `nightmoth.exe`                 the app (window, tray, pop-ups) — normal user rights
//! - `nightmoth.exe --engine`        the engine (all system changes + Autopilot) — admin
//! - `nightmoth.exe --setup-engine`  one-time, elevated: registers the engine's scheduled task

mod api;
mod apps;
mod apptheme;
mod autopilot;
mod changelog;
mod config;
mod deepclean;
mod fixer;
mod gallery;
mod games;
mod history;
mod ipc;
mod log;
mod monitors;
mod organize;
mod look;
mod media;
mod net;
mod roblox;
mod speed;
mod state;
mod storage;
mod stats;
mod toast;
mod ui_toast;
mod updates;
mod upkeep;
mod taskbar;
mod tidy;
mod userjob;
mod widgets;
mod win;

use serde_json::{json, Value};
use std::time::Instant;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

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
        win::set_elevated_role();
        log::set_role("engine");
        run_engine();
    } else if args.iter().any(|a| a == "--setup-engine") {
        win::set_elevated_role();
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
    // 0.3: the taskbar no longer hides; it stays put with a see-through background
    if !state::cfg().taskbar_migrated {
        if win::taskbar_autohide() {
            let _ = look::desktop_set("autohide", &json!(false));
        }
        state::with_cfg(|c| c.taskbar_migrated = true);
    }
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
        .inner_size(ui_toast::W, ui_toast::H)
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
    let perfect = MenuItem::with_id(app, "perfect", "Make my PC perfect…", true, None::<&str>)?;
    let organize_now = MenuItem::with_id(app, "organize", "Organize desktop now", true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next_wall", "Next wallpaper", true, None::<&str>)?;
    let invisible = CheckMenuItem::with_id(app, "invisible", "Invisible taskbar", true, true, None::<&str>)?;
    let widgets_item = CheckMenuItem::with_id(app, "widgets", "Desktop widgets", true, true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open, &perfect, &optimize, &organize_now, &next, &sep2, &invisible, &widgets_item, &autopilot, &sep, &quit],
    )?;

    // show the real states once the engine answers
    let (ap_init, inv_init, wid_init) = (autopilot.clone(), invisible.clone(), widgets_item.clone());
    std::thread::spawn(move || {
        if let Ok(c) = ipc::call("get_config", json!({})) {
            let _ = ap_init.set_checked(c["autopilot"].as_bool().unwrap_or(true));
            let _ = inv_init.set_checked(c["taskbar_style"].as_str().unwrap_or("clear") != "normal");
            let _ = wid_init.set_checked(c["widgets_on"].as_bool().unwrap_or(true));
        }
    });
    let (inv, wid) = (invisible.clone(), widgets_item.clone());

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
            "perfect" => {
                show_main(app);
                let _ = app.emit_to("main", "open-page", "smart");
            }
            "organize" => {
                std::thread::spawn(|| match organize::run(std::time::Duration::ZERO) {
                    Ok(m) if !m.is_empty() => ui_toast::local("Desktop organized", &format!("{} apps sorted into folders", m.len())),
                    Ok(_) => ui_toast::local("Desktop organized", "Everything was already in its folder"),
                    Err(e) => ui_toast::local("Couldn't organize", &e),
                });
            }
            "invisible" => {
                let inv = inv.clone();
                std::thread::spawn(move || {
                    let on = inv.is_checked().unwrap_or(true);
                    let _ = ipc::call("desktop_set", json!({ "key": "taskbar", "value": if on { "clear" } else { "normal" } }));
                });
            }
            "widgets" => {
                let wid = wid.clone();
                std::thread::spawn(move || {
                    let on = wid.is_checked().unwrap_or(true);
                    let _ = ipc::call("set_config", json!({ "patch": { "widgets_on": on } }));
                });
            }
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
fn widgets_ready() {
    widgets::page_ready();
}

#[tauri::command]
fn widgets_size(w: f64, h: f64) {
    widgets::set_size(w, h);
}

#[tauri::command]
fn tidy_preview() -> Value {
    tidy::preview()
}

#[tauri::command]
async fn organize_preview() -> Value {
    tauri::async_runtime::spawn_blocking(organize::preview).await.unwrap_or(Value::Null)
}

#[tauri::command]
async fn organize_run() -> Result<usize, String> {
    let moved = tauri::async_runtime::spawn_blocking(|| organize::run(std::time::Duration::ZERO)).await.map_err(|e| e.to_string())??;
    let n = moved.len();
    let mut kinds: Vec<&str> = moved.iter().map(|m| m.1.as_str()).collect();
    kinds.sort();
    kinds.dedup();
    ui_toast::local("Desktop organized", &format!("{n} apps sorted into {} folders", kinds.len()));
    Ok(n)
}

#[tauri::command]
fn organize_undo() -> Result<usize, String> {
    let n = organize::undo()?;
    ui_toast::local("Desktop put back", &format!("{n} apps back where they were"));
    Ok(n)
}

#[tauri::command]
fn organize_set_kind(file: String, kind: String) -> Result<(), String> {
    organize::set_kind(&file, &kind)
}

#[tauri::command]
fn tidy_run() -> Result<usize, String> {
    let n = tidy::run(std::time::Duration::ZERO)?;
    ui_toast::local("Desktop tidied", &format!("{n} files sorted into folders"));
    Ok(n)
}

#[tauri::command]
fn tidy_undo() -> Result<usize, String> {
    let n = tidy::undo()?;
    ui_toast::local("Desktop restored", &format!("{n} files put back"));
    Ok(n)
}

// ---- 0.4 app-side tools (run with your own rights)

#[tauri::command]
async fn storage_scan() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| serde_json::to_value(storage::scan()).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn storage_recycle(paths: Vec<String>) -> Result<usize, String> {
    storage::recycle(&paths)
}

#[tauri::command]
async fn roblox_state() -> Value {
    tauri::async_runtime::spawn_blocking(roblox::state).await.unwrap_or(Value::Null)
}

#[tauri::command]
async fn roblox_apply() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(|| roblox::apply(true)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn roblox_restore() -> Result<(), String> {
    roblox::restore()
}

#[tauri::command]
fn monitors_list() -> Result<Vec<Value>, String> {
    monitors::list()
}

#[tauri::command]
async fn monitor_set(id: String, path: String, primary: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if primary {
            // the main screen drives the theme
            ipc::call("look_apply", json!({ "path": path }))?;
        } else {
            monitors::set(&id, &path)?;
        }
        let mut walls = ipc::call("get_config", json!({}))?["monitor_walls"].as_object().cloned().unwrap_or_default();
        walls.insert(id, json!(if primary { String::new() } else { path }));
        ipc::call("set_config", json!({ "patch": { "monitor_walls": walls } }))?;
        if primary {
            monitors::reapply(&walls);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn media_control(action: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || media::control(&action)).await.map_err(|e| e.to_string())?
}

/// Start an app from the dock widget (only apps that are on the dock).
#[tauri::command]
fn dock_launch(path: String) -> Result<(), String> {
    let c = ipc::call_quiet("get_config", json!({}))?;
    if !c["dock"].as_array().map(|d| d.iter().any(|x| x.as_str() == Some(path.as_str()))).unwrap_or(false) {
        return Err("Not on the dock".into());
    }
    use std::os::windows::process::CommandExt;
    std::process::Command::new("cmd.exe")
        .raw_arg(format!("/C start \"\" \"{path}\""))
        .creation_flags(win::CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn look_export_file(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let v = ipc::call("look_export", json!({}))?;
        std::fs::write(&path, serde_json::to_vec(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn look_import_file(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
        if meta.len() > 40 * 1024 * 1024 {
            return Err("That file is too big to be a look".into());
        }
        let v: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?).map_err(|_| "That isn't a Nightmoth look file".to_string())?;
        ipc::call("look_import", json!({ "data": v })).map(|n| n.as_str().unwrap_or("").to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn cursor_set(on: bool, accent: String) -> Result<(), String> {
    ipc::call_quiet("set_config", json!({ "patch": { "accent_cursor": on } }))?;
    if on {
        apptheme::set_cursor(&accent)
    } else {
        apptheme::reset_cursor();
        Ok(())
    }
}

#[tauri::command]
fn taskbar_status() -> String {
    taskbar::helper_status()
}

#[tauri::command]
fn theme_apps_now(accent: String) -> usize {
    apptheme::theme_apps(&accent)
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
            userjob::start_app_worker();
            widgets::start(app.handle().clone());
            tidy::start_auto();
            organize::start_auto();
            let helper = app.path().resolve("nightmoth_tap.dll", tauri::path::BaseDirectory::Resource).ok();
            taskbar::start(helper);
            roblox::start_auto();
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
                        userjob::start_delayed_apps();
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
            widgets_ready,
            widgets_size,
            tidy_preview,
            tidy_run,
            tidy_undo,
            organize_preview,
            organize_run,
            organize_undo,
            organize_set_kind,
            storage_scan,
            storage_recycle,
            roblox_state,
            roblox_apply,
            roblox_restore,
            monitors_list,
            monitor_set,
            media_control,
            dock_launch,
            look_export_file,
            look_import_file,
            cursor_set,
            theme_apps_now,
            taskbar_status,
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
