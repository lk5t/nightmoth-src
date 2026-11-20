# Nightmoth (source — private)

Windows 11 desktop glow-up + autopilot optimizer. See [SPEC.md](SPEC.md).

- **Stack:** Tauri 2 (Rust) + TypeScript/Vite
- **Download:** every push to `main` builds the installer and puts it in this repo's **Releases → "Nightmoth (latest build)"**.
- **Versions:** push a tag like `v0.1.0` for a versioned release.

## Build locally (Windows)

```
npm install
npx tauri dev     # run
npx tauri build   # installer in src-tauri/target/release/bundle/nsis/
```

Needs Rust (stable), Node 22, and the Visual Studio C++ build tools. The build also compiles the taskbar helper in `src-tauri/tap` and ships it as a resource.

## Two processes

- **App** (`nightmoth.exe`, normal user): window, tray, pop-ups, widgets, taskbar look, desktop tidy. Files in `%APPDATA%\Nightmoth`.
- **Engine** (`nightmoth.exe --engine`, admin, started by the "Nightmoth Engine" scheduled task): every system change and Autopilot. Files in `%ProgramData%\Nightmoth`, which only admins can write to.
- The engine never runs anything a normal program could have planted (per-user uninstallers, relaunching apps closed for a game) — those go back to the app as user jobs.

## Layout

| Path | What |
|---|---|
| `src-tauri/src/win.rs` | Registry, PowerShell, wallpaper, taskbar, desktop icons, priority |
| `src-tauri/src/look.rs` | Wallpaper analysis + theme engine, taskbar/desktop, presets, slideshow |
| `src-tauri/src/gallery.rs` | Generated "Nightmoth Originals" wallpapers |
| `src-tauri/src/speed.rs` | Startup, cleanup, debloat, services, privacy, power, boot time |
| `src-tauri/src/apps.rs` | Installed apps, usage, automatic unused-app cleaner |
| `src-tauri/src/games.rs` | Game detection + boost/restore |
| `src-tauri/src/autopilot.rs` | Background loop (hogs, cleanup schedule, weekly report) |
| `src-tauri/src/toast.rs` | Bottom-right pop-ups (engine queue; always on) |
| `src-tauri/src/ui_toast.rs` | Shows the pop-ups bottom-right (app side) |
| `src-tauri/src/widgets.rs` | Desktop widgets: clock, calendar, system, week (app side) |
| `src-tauri/src/taskbar.rs` | Invisible / tinted-glass taskbar background (app side) |
| `src-tauri/src/tidy.rs` | Tidy desktop: sort loose files into folders, undo (app side) |
| `src-tauri/src/userjob.rs` | Jobs the engine hands to the app so they run *without* admin rights |
| `src-tauri/src/ipc.rs` | App ↔ engine link (only Nightmoth's own exe is answered) |
| `src-tauri/src/upkeep.rs` | "Run when needed": low disk, junk piling up, away from PC, battery, visuals, update hours |
| `src-tauri/src/fixer.rs` | Finds and repairs damage left by other optimizer tools |
| `src-tauri/src/deepclean.rs` | Old restore points, drivers, app leftovers, Windows.old, GPU installer junk… |
| `src-tauri/src/updates.rs` | App updates through winget |
| `src-tauri/src/net.rs` | DNS benchmark/switch, ping |
| `src-tauri/src/history.rs` | Daily snapshots for the Progress page |
| `src-tauri/src/storage.rs` | Biggest files, duplicates, old downloads (app side, Recycle Bin) |
| `src-tauri/src/roblox.rs` | Roblox graphics/FPS for your hardware (app side) |
| `src-tauri/src/apptheme.rs` | Terminal / VS Code theming, accent pointer (app side) |
| `src-tauri/src/monitors.rs` | Wallpaper per screen (app side) |
| `src-tauri/src/media.rs` | Now-playing widget (app side) |
| `src-tauri/tap/` | Taskbar helper DLL loaded into Explorer for the fully invisible taskbar (Windows 11) |
| `src-tauri/src/changelog.rs` | Every change recorded + undo |
| `src/` | UI (main window + pop-up window) |
