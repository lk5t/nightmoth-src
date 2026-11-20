# NIGHTMOTH — Master Spec

> Windows 11 desktop glow-up + autopilot optimizer.
> Pick a wallpaper → Nightmoth themes the whole PC around it, then quietly keeps the machine fast from the system tray.

- **Platform:** Windows 11 (x64) only
- **Stack:** Tauri 2 (Rust backend + web frontend)
- **Source:** `lk5t/nightmoth-src` (private)
- **Releases:** `lk5t/Nightmoth` (public — README, LICENSE, screenshots, `.exe` installer in Releases only)
- **App look:** black and red, fixed (the app does NOT re-colour with the wallpaper)

---

## 1. Principles

1. **Automatic by default.** Nightmoth lives in the tray and acts on its own. The user should rarely need to open the window.
2. **Only real optimizations.** No registry cleaners, no "RAM boosters". Every speed feature must be measurable (boot time, disk freed, background CPU).
3. **Everything is reversible.** Every change is logged with enough data to undo it. Restore point before any risky batch.
4. **Tell, don't nag.** Actions are announced with a 5-second toast, never a blocking dialog (except first-run setup).
5. **Fast and light.** Idle target: < 60 MB RAM, ~0% CPU. The optimizer must not become the bloat.

---

## 2. App design (black & red)

### Palette
| Token | Hex | Use |
|---|---|---|
| `bg` | `#0A0A0B` | Window background |
| `panel` | `#121214` | Cards, sidebar |
| `panel-2` | `#1A1A1D` | Hover, inputs |
| `border` | `#26262A` | Hairlines |
| `text` | `#F2F2F3` | Primary text |
| `text-dim` | `#8A8A92` | Secondary text |
| `red` | `#E10600` | Primary accent, active tab, toggles on |
| `red-glow` | `#FF2D3A` | Hover, glows, progress bars |
| `red-deep` | `#5A0A0C` | Accent backgrounds, selected rows |
| `ok` | `#3DDC84` | Success numbers only |
| `warn` | `#FFB020` | Warnings only |

### Style
- Frameless window, custom title bar, Windows 11 Mica disabled (solid black so the red pops)
- Rounded 10px cards, 1px borders, subtle red glow on focus/active
- Font: Inter (UI), JetBrains Mono (numbers/stats)
- Smooth 150–250ms transitions; no bouncy animations
- Moth logo: minimal geometric moth outline in red

### Layout
```
┌──────────┬───────────────────────────────────────┐
│  MOTH    │  Title / status bar                   │
│  logo    ├───────────────────────────────────────┤
│          │                                       │
│ ◉ Home   │                                       │
│ ○ Look   │            Active tab                 │
│ ○ Speed  │                                       │
│ ○ Auto-  │                                       │
│   pilot  │                                       │
│ ○ Games  │                                       │
│ ○ Safety │                                       │
│          │                                       │
│ ⚙ Setts  │                                       │
└──────────┴───────────────────────────────────────┘
```
Sidebar with tabs. Active tab = red bar on the left + red icon.

---

## 3. Tabs

### 3.1 Home
- PC health score (0–100) with the moth logo glowing red/white
- Live stats: CPU, GPU, RAM, disk, temps (where available)
- "This week" card: GB freed, apps removed, games boosted, boot time change
- Current wallpaper + its generated theme preview
- Big **Optimize now** button (runs everything in Speed once)

### 3.2 Look — wallpaper-driven theming
**Wallpaper sources**
- User's own photos (file or folder)
- Built-in gallery of free dark wallpapers (bundled CC0 set + optional online gallery later)
- Slideshow: folder + interval (15 min / 1 h / daily / time-of-day)

**Theme engine** (runs automatically every time the wallpaper changes)
1. Analyse image (Rust, downscaled to ~256px):
   - Dominant palette: k-means, 6 colours
   - Average luminance → dark or light mode
   - Saturation → style (muted / vivid / neon)
   - Busy-region map (edge density) → where empty space is
2. Pick accent: most "characterful" palette colour that passes contrast (WCAG ≥ 3:1 against taskbar/Start backgrounds). Adjust lightness if it fails.
3. Apply to Windows:
   - Wallpaper (`SystemParametersInfoW` `SPI_SETDESKWALLPAPER`)
   - Dark/light mode (`AppsUseLightTheme`, `SystemUsesLightTheme`)
   - Accent colour (`Explorer\Accent\AccentPalette`, `AccentColorMenu`, `StartColorMenu`; `DWM\AccentColor`)
   - Accent on Start/taskbar and title bars (`ColorPrevalence`)
   - Lock screen (same image)
   - Broadcast `WM_SETTINGCHANGE` ("ImmersiveColorSet") so it applies live
   - Re-verify after 3 s and on Explorer restart (Windows sometimes reverts accent)
4. Toast: "New theme applied from your wallpaper"

**Controls**
- Lock accent colour (keep a favourite across wallpapers)
- Shuffle accent (pick another palette colour)
- Saved looks: each wallpaper remembers its theme

**Desktop & taskbar**
- Taskbar auto-hide (`SHAppBarMessage` `ABM_SETSTATE`)
- Taskbar alignment (`TaskbarAl`), hide Search / Widgets / Task View / Chat buttons
- Hide desktop icons (toggle desktop view `HideIcons` + refresh)
- Window corner style (rounded / square) where supported

**Presets:** Void · Neon Night · Clean · Custom (save your own)

### 3.3 Speed (manual tools, also used by Autopilot)
| Module | What it does |
|---|---|
| **Startup manager** | Lists every autostart location (Run keys HKCU/HKLM, Startup folders, scheduled tasks at logon, services set to auto). Shows impact (est. boot delay). Disable/enable/delete. |
| **Cleanup** | `%TEMP%`, `C:\Windows\Temp`, Windows Update cache (`SoftwareDistribution\Download`), Delivery Optimization cache, crash dumps, old logs, thumbnail cache, browser caches (Chrome/Edge/Brave/Firefox, closed browsers only). Shows size before cleaning. |
| **Debloat** | Remove preinstalled UWP bloat (`Remove-AppxPackage` + deprovision). Curated list, each item explained. |
| **Services** | Curated safe-to-disable list only (e.g. DiagTrack, dmwappushservice, RetailDemo, Xbox services if no Xbox use, Fax). Never blanket-disable. |
| **Privacy & ads** | Telemetry level, advertising ID, Start menu suggestions, lock-screen tips, File Explorer ads, Bing in Start search. Plain-language toggles. |
| **Power & GPU** | Ultimate Performance power plan (create if missing), hardware-accelerated GPU scheduling (`HwSchMode`), Windows Game Mode on. |
| **Boot timer** | Reads boot duration from Event Log (Diagnostics-Performance, event 100) → before/after chart. |

### 3.4 Autopilot (tray, fully automatic)
Master toggle: **Autopilot ON/OFF**. Each module has its own toggle and schedule.

**Unused app cleaner — fully automatic**
- Collect installed apps: Uninstall registry keys (HKLM, HKLM WOW6432, HKCU) + Appx packages
- Usage data per app: Prefetch last-run times, UserAssist (ROT13-decoded), Amcache, exe last-access as fallback. Map exes to apps via `InstallLocation`.
- Rule: unused for **N days** (default 90; options 60 / 90 / 180)
- **Safeguards (cannot be turned off):**
  - Protected list: system components, drivers, runtimes (VC++, .NET, DirectX), security software, Nightmoth, game launchers, anything user marks **Keep**
  - Skip anything installed in the last 30 days
  - **5-second cancel window**: toast "Removing X in 5s — unused 94 days" with **Cancel** (cancel = auto-added to Keep)
  - Removal log with **Reinstall** button (winget ID matched where available)
- Uninstall method: `QuietUninstallString` → `msiexec /x {GUID} /qn` → `Remove-AppxPackage`. If no silent method exists, skip and list it as "needs manual removal".

**Game booster**
- Detect game launch: process-start events (WMI `Win32_ProcessStartTrace`) matched against:
  - Known-games list (Roblox, Steam/Epic/Battle.net/Riot/EA game folders)
  - Steam/Epic library folders
  - Fallback: exclusive/borderless fullscreen on primary monitor for > 10 s
- On launch: Ultimate Performance plan, game process `HIGH_PRIORITY_CLASS`, suspend/close user-defined background apps, Focus Assist / Do Not Disturb on, pause slideshow + heavy Nightmoth work
- On exit: restore everything exactly
- **Per-game best settings (later phase):** for supported games, edit the game's own settings file based on detected hardware (GPU tier, RAM, refresh rate). Back up the original file first. Start with Roblox, then add games one at a time.

**Always-on upkeep**
- Scheduled cleanup (default weekly, Sunday 03:00, or on idle)
- Startup watcher: new autostart entry → disabled automatically if not on the allowlist → toast
- Background hog watcher: process using > 25% CPU or > 1.5 GB RAM for > 5 min while not in foreground → toast (and optional auto-lower priority)
- Weekly report toast + Home card

### 3.5 Games
- List of detected games with last played, boost on/off per game
- Per-game: background apps to close, priority, settings profile (when supported)
- Boost history

### 3.6 Safety
- **Change log:** every change Nightmoth made (time, module, before → after), each with **Undo**
- **Restore point** automatically before Debloat / Services / batch uninstall (System Protection must be on — offer to enable it)
- **Panic button:** revert all Nightmoth changes
- Protected/Keep list editor

### 3.7 Settings
- Start with Windows (on by default)
- Toast settings: on/off per type, duration (default 5 s), quiet mode
- Unused-app threshold, cleanup schedule, hog thresholds
- Update check (GitHub Releases of `lk5t/Nightmoth`)
- About / license

---

## 4. Toasts (Razer Cortex style)
- Bottom-right of primary monitor, above taskbar area, 360×88 px
- Black card `#0A0A0B`, red left edge + red countdown bar, moth icon, title + one line
- Slides in 200 ms, stays **5 s**, fades out; hover pauses; click opens details
- Optional action button (e.g. **Cancel**, **Undo**)
- Separate always-on-top, no-activate, click-through-except-card Tauri window
- Never shown while a game is in exclusive fullscreen, except the launch/exit boost toasts

Examples:
- `Roblox boosted` — 7 apps paused · max power on
- `Back to normal` — everything restored
- `Cleanup done` — freed 2.4 GB
- `Startup blocked` — XYZUpdater
- `Removing Spotify in 5s` — unused 94 days · [Cancel]
- `Theme applied` — from your new wallpaper

---

## 5. Architecture

```
nightmoth-src/
├─ src-tauri/            Rust
│  ├─ core/              config, change log, undo engine, scheduler
│  ├─ look/              palette extraction, theme apply, taskbar, icons, wallpaper
│  ├─ speed/             startup, cleanup, debloat, services, privacy, power
│  ├─ autopilot/         unused apps, game booster, upkeep, watchers
│  ├─ toast/             toast window manager
│  ├─ win/               windows-rs wrappers (registry, WMI, shell, DWM, power)
│  └─ main.rs            tray, windows, IPC commands
├─ src/                  frontend (Svelte or React + TS, Tailwind)
│  ├─ tabs/  components/  toast/  theme.css
├─ assets/wallpapers/    bundled CC0 wallpapers
└─ installer/            NSIS config, license text
```

**Key crates:** `windows` (Win32/WinRT), `winreg`, `wmi`, `image` + `kmeans_colors`, `sysinfo`, `tokio`, `serde`, `rusqlite` (change log + usage DB), `tauri-plugin-autostart`, `tauri-plugin-single-instance`, `tauri-plugin-updater`.

**Elevation:** many modules need admin (services, HKLM, Prefetch, uninstall, WMI process trace).
- Tray app runs **elevated at logon** through a Task Scheduler task ("Run with highest privileges") so there's no UAC prompt every boot.
- First run asks for admin once to create that task.

**Storage:** `%APPDATA%\Nightmoth\` — `config.json`, `nightmoth.db` (change log, usage, removal log), `looks/` (saved themes), `backups/` (original registry values + game config files).

---

## 6. Distribution
- Build in `nightmoth-src` → NSIS installer `Nightmoth-Setup-x.y.z.exe`
- Upload to **GitHub Releases on `lk5t/Nightmoth`** (public repo has no source)
- Public repo contains: README (features, screenshots, install steps, SmartScreen note), LICENSE, CHANGELOG
- **License:** proprietary freeware — free to use, no redistribution, no modification, no rebranding, no reverse engineering
- Unsigned at first: users see SmartScreen "More info → Run anyway" (documented in README). Code signing later.
- Auto-update via `tauri-plugin-updater` reading the public repo's latest release

---

## 7. Build phases

| Phase | Scope | Done when |
|---|---|---|
| **1. Skeleton** | Tauri 2 app, black/red UI shell with all tabs, tray icon, single instance, start with Windows, elevated logon task, config + SQLite change log + undo engine | App installs, lives in tray, tabs navigate, a test change can be undone |
| **2. Toasts** | Toast window system, countdown, actions, settings | Any module can fire a toast with an optional Cancel/Undo |
| **3. Look** | Wallpaper picker, bundled gallery, palette engine, auto-theme on change, dark/light, accent, taskbar auto-hide + buttons, hide icons, presets, saved looks | Picking a wallpaper re-themes Windows live; every change undoable |
| **4. Speed** | Startup manager, cleanup, debloat, services, privacy, power/GPU, boot timer | Each module works manually with before/after numbers |
| **5. Autopilot** | Scheduler, unused app cleaner (full auto + safeguards), startup watcher, hog watcher, weekly report | Runs a week unattended without breaking anything |
| **6. Game booster** | Game detection, boost/restore, Games tab, per-game options | Launching Roblox boosts and closing it restores, with toasts |
| **7. Slideshow & polish** | Wallpaper slideshow with theme follow, Home dashboard, health score, animations | Feels finished |
| **8. Release** | Installer, public README/LICENSE, updater, first GitHub release | `.exe` downloadable from `lk5t/Nightmoth` Releases |
| **9. Later** | Per-game best settings (Roblox first), desktop widgets, online wallpaper gallery, code signing | — |

---

## 8. Out of scope (on purpose)
- Registry cleaning
- RAM "boosters"
- Blanket service disabling
- Driver installation/modification
- Theming non-Windows apps (Terminal, VS Code, Discord…) — maybe later
