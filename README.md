# Nightmoth (source — private)

Windows 11 desktop glow-up + autopilot optimizer. See [SPEC.md](SPEC.md).

- **Stack:** Tauri 2 (Rust) + TypeScript/Vite
- **Builds:** every push to `main` builds the installer on GitHub Actions (Actions tab → latest run → `Nightmoth-installer`).
- **Releases:** push a tag like `v0.1.0` to publish the `.exe` to the public repo `lk5t/Nightmoth` (needs the `NIGHTMOTH_RELEASE_TOKEN` secret).

## Build locally (Windows)

```
npm install
npx tauri dev     # run
npx tauri build   # installer in src-tauri/target/release/bundle/nsis/
```

Needs Rust (stable), Node 22, and the Visual Studio C++ build tools.

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
| `src-tauri/src/toast.rs` | Bottom-right pop-ups |
| `src-tauri/src/changelog.rs` | Every change recorded + undo |
| `src/` | UI (main window + pop-up window) |
