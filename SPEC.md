# NIGHTMOTH — Master Spec

> Windows 11 desktop glow-up + autopilot optimizer.
> Pick a wallpaper → Nightmoth themes the whole PC around it, then quietly keeps the machine fast from the system tray.

- **Platform:** Windows 11 (x64) only
- **Stack:** Tauri 2 (Rust backend + web frontend)
- **Repo:** `lk5t/nightmoth-src` — source and the `.exe` installer (in Releases), all in one place
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
| `bg` | `#060607` | Window background |
| `panel` | `#0D0D0F` | Cards, sidebar |
| `panel-2` | `#131316` | Hover, inputs |
| `border` | `#1C1C20` | Hairlines |
| `text` | `#F2F2F3` | Primary text |
| `text-dim` | `#8A8A92` | Secondary text |
| `red` | `#A3121A` | Primary accent, toggles on, primary buttons (deep crimson, not signal red) |
| `red-glow` | `#C8222B` | Hover, active tab bar, progress bars |
| `red-text` | `#E0474F` | Red text on black |
| `red-deep` | `#330A0D` | Accent backgrounds, selected rows |
| `ok` | `#3DDC84` | Success numbers only |
| `warn` | `#FFB020` | Warnings only |

### Style
- Frameless window, custom title bar, Windows 11 Mica disabled (solid black so the red pops)
- Rounded 12px cards, 1px borders, almost no glow — red is used sparingly
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
- Taskbar background: **Invisible** (default) / **Tinted glass** / Windows default — `SetWindowCompositionAttribute` re-applied by the app; the taskbar is never hidden (auto-hide is switched off once on upgrade)
- **Desktop widgets** (Rainmeter / Seelen UI / BeWidgets idea): clock, calendar, system monitor, Nightmoth week. One see-through, click-through window behind all windows. Accent from the wallpaper, placed automatically in its calmest area (or a chosen spot). Hidden while gaming.
- **Tidy desktop** (Fences idea): sort loose desktop files into Images / Videos / Music / Documents / Archives / Installers / Code / Other; shortcuts and folders stay. Undo last tidy. Autopilot does it daily for files untouched for a day.
- Taskbar alignment (`TaskbarAl`), hide Search / Widgets / Task View / Chat buttons
- Hide desktop icons (toggle desktop view `HideIcons` + refresh)
- Window corner style (rounded / square) where supported

**Presets:** Void (invisible taskbar) · Neon Night (tinted glass) · Clean (invisible) · Windows default

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
- Usage data per app — all recorded by Windows itself, so it covers the time before Nightmoth was installed: Prefetch last-run times, UserAssist (ROT13-decoded, exes and Start-menu shortcuts), BAM (`bam\State\UserSettings`). Map exes to apps via `InstallLocation` and `DisplayIcon`.
- Rule: unused for **N days** (default 90; options 60 / 90 / 180)
- **Safeguards (cannot be turned off):**
  - Protected list: system components, drivers, runtimes (VC++, .NET, DirectX), security software, Nightmoth, game launchers, anything user marks **Keep**
  - Skip anything installed in the last 30 days
  - **5-second cancel window**: toast "Removing X in 5s — unused 94 days" with **Cancel** (cancel = auto-added to Keep)
  - Removal log with **Reinstall** button (winget ID matched where available)
- Uninstall method: `QuietUninstallString` → `msiexec /x {GUID} /qn` → Inno `/VERYSILENT` → NSIS `/S` → Squirrel `--uninstall -s`. If no silent method exists, skip and list it as "needs manual removal".
- Per-user (HKCU) uninstallers run with the user's rights through the app, never elevated.
- Leftover install folder deleted after a successful uninstall, only if it is a dedicated folder under Program Files / LocalAppData\Programs / AppData and no other installed app lives there.

**Game booster**
- Detect game launch: process-start events (WMI `Win32_ProcessStartTrace`) matched against:
  - Known-games list (Roblox, Steam/Epic/Battle.net/Riot/EA game folders)
  - Steam/Epic library folders
  - Fallback: exclusive/borderless fullscreen on primary monitor for > 10 s
- On launch: Ultimate Performance plan, game process `HIGH_PRIORITY_CLASS`, suspend/close user-defined background apps, Focus Assist / Do Not Disturb on, pause slideshow + heavy Nightmoth work
- On exit: restore everything exactly (closed apps are reopened as the normal user)
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
- Pop-ups are always on (no per-type or quiet switches); only the duration can change
- Unused-app threshold, cleanup schedule, hog thresholds
- Update check (GitHub Releases of `lk5t/Nightmoth`)
- About / license

---

### 3.8 Added in 0.4

**Autopilot runs when it's needed, not on a timetable** (`upkeep.rs`): cleanup when the system drive is under 10 % / 15 GB free or junk passes 1.5 GB; heavier work (deep clean, app updates, DNS re-test, daily snapshot) only while the user has been away 10+ minutes, not gaming, and plugged in or above 60 % battery.

**Repair other tools' damage** (`fixer.rs`, automatic, undoable): disabled core services (Windows Update, BITS, Defender UI, Store, Search, audio…), policies that switch off updates/Defender/SmartScreen, UAC off, Spectre/Meltdown mitigations off, power throttling off, page file removed, `bcdedit` timer "tweaks", TCP auto-tuning off, update/Store/activation servers blocked in `hosts`, Defender exclusions covering whole drives. Good leftovers (telemetry blocked, Start ads off) are kept and listed. Restore point first.

**Deep clean** (`deepclean.rs`, deleted straight away): old restore points (newest kept), DISM component cleanup, old driver versions (`pnputil` without `/force`, so drivers in use stay), folders/settings/registry keys of apps that were uninstalled (only exact name matches, not touched for 7 days, never shared vendors), C:\NVIDIA / C:\AMD / old driver downloads, Windows.old after 10 days, old servicing logs and MEMORY.DMP, Discord/Steam/Epic web caches while closed, installers in Downloads older than 60 days.

**Speed extras**: winget updates; delayed startup (app starts listed apps 60 s after login); Efficiency mode (EcoQoS + idle priority) for background hogs; lighter visual effects on PCs under 8 GB / ≤ 4 threads; battery mode (Balanced plan, taskbar effect paused, widgets slowed); Windows Update active hours 08:00–02:00 and Delivery Optimization throttled + update services stopped while gaming; Roblox graphics level from GPU tier and FPS cap = refresh rate (only allow-listed settings, originals backed up, re-applied after Roblox updates).

**Network & storage**: DNS benchmark over 12 common domains against Cloudflare/Google/Quad9/OpenDNS/AdGuard and the current server, switch with exact undo; ICMP ping; storage finder for files ≥ 500 MB, same-content duplicates ≥ 20 MB, Downloads untouched for 90 days (Recycle Bin).

**Progress page**: daily idle snapshot (score, boot time, startup apps, idle RAM/CPU, junk, free space, app count); first vs latest; score history; this week vs last.

**Customization**: widgets for now playing (with controls), weather (open-meteo, city chosen by the user), notes, app dock; Windows Terminal scheme + VS Code accent colours; accent mouse pointer (generated .cur); accent folder icon (Shell Icons 3/4); look export/import (`.nightmoth` file with the wallpaper inside); per-screen wallpapers (IDesktopWallpaper, main screen drives the theme); fully invisible taskbar on Windows 11 via the XAML Diagnostics helper (`tap/`), which fades out the `BackgroundFill`/`BackgroundStroke` layers, follows `HKCU\Software\Nightmoth\TaskbarClear`, and is disabled automatically after two Explorer restarts right after loading.

### 3.9 Added in 0.5 — Organize desktop

Every app on the desktop (shortcuts, internet shortcuts, programs that aren't setup files — on your desktop and the shared Public desktop) goes into a folder for its kind: Games, Social, Browsers, Music & Video, Creative, Work, Dev tools, System & utilities, Other apps. Nothing is left out: unclear apps go to "Other apps".

Kind is decided by, in order: the user's own choice (a folder picked in Nightmoth, or an app moved by hand — learned), a large known-apps list (longest keyword wins), launcher links (`steam://`, Epic, Riot, Battle.net…) and game-library install folders, website domains for `.url` files, the exe's CompanyName/FileDescription, game-engine files next to the exe, and the publisher of the matching uninstall entry.

Public desktop shortcuts are moved by the engine (admin) only into the user's own desktop category folders, with the same checks on the way back. "Organize automatically" (Autopilot) sorts new apps within a minute (shortcuts younger than 45 s are left until the installer finishes). The last organize can be undone. Tidy desktop leaves app programs to the organizer.

### 3.10 Added in 0.6 — one click, more automatic

- **Make my PC perfect** (Home, tray, first-run welcome): a live checklist that runs restore point → repair other tools' damage → junk cleanup → block ads & tracking → safe services off → delay heavy startup apps → Game Mode, night-only update restarts, lighter visuals on weak PCs → fastest DNS (only if clearly faster) → every Autopilot switch on (incl. organize desktop, widgets, invisible taskbar, app theming) and the theme re-applied → organize desktop apps → Roblox tuned. Nothing is uninstalled; everything is undoable.
- **Every game boosted**: anything other than browsers/players/tools that stays fullscreen or borderless-fullscreen for 10 s is boosted and added to the Games list (where it can be switched off).
- **Startup guard delays instead of blocks** heavy apps that add themselves (Discord, Steam, Spotify…), with "Start normally" on the pop-up; other new entries are still blocked with "Allow".
- **Autopilot activity feed** on Home.
- **Tray**: Make my PC perfect, Organize desktop now, Invisible taskbar and Desktop widgets switches.

### 3.12 Changed in 0.8 — Organize desktop without folders

Apps are no longer moved into folders. Every icon stays on the desktop and is positioned through the desktop's own icon view (`IShellWindows` → `IFolderView2::SelectAndPositionItems`, auto-arrange switched off): Windows' icons (Recycle Bin, This PC) first, then one compact block per kind (up to 5 tall, alphabetical), then other files and folders, with a one-space gap between blocks. Blocks run down the calmer side of the wallpaper (left/right column busyness from the wallpaper analysis, avoiding the widgets' side) and continue in the next column when one fills up. Shared-desktop shortcuts are placed the same way without being moved. Folders made by 0.5–0.7 are emptied back onto the desktop. Undo restores every icon's previous position. Choosing a group for an app re-arranges and is remembered; Autopilot places new apps within a minute.

### 3.11 Added in 0.7 — removal that doesn't give up

- **Uninstall chain** (unused-app cleaner and Remove button): the app's QuietUninstallString / known silent command → `MsiExec /X{product code}` (from the uninstall string or key name) → common silent switches on the uninstaller (`/S`, `/VERYSILENT…`, `/quiet`, `/silent`, `-s`, `--silent`, …, fast time limits) → `winget uninstall --exact --silent --force` → forced removal (close its processes, delete its own install folder, Start menu/desktop shortcuts pointing into it, and its uninstall entry; only for a dedicated folder that passes the leftover-folder checks). Every step is time-limited and verified before moving on. Per-user apps still run everything with the user's rights. The cleaner no longer needs a silent uninstaller to pick an app.
- **Store junk**: longer list (Bing apps, Tips, Feedback Hub, Clipchamp, Cortana, Skype, 3D apps, Mixed Reality, Mobile Plans, LinkedIn, King/Gameloft/Zynga games…). Removal tries all users → per package → per package for all users → provisioned package → DISM, and checks after each. Autopilot ("Remove junk apps", on) removes the ones marked automatic once a day and again after Windows brings them back; apps people may use (Phone Link, To Do, Teams, streaming/social stubs) stay manual.
- **Files**: deletes that fail are retried after clearing read-only/hidden/system flags, then (engine) after taking ownership and granting Administrators; links inside the folder are removed first so nothing leads elsewhere; whatever is still locked is deleted at the next restart (`MoveFileEx` delay-until-reboot). Used by cleanup and deep clean.
- **Broken things**: Run-key startup entries whose program no longer exists (undoable) and Start menu/desktop shortcuts pointing at deleted programs, in deep clean and Make my PC perfect.

## 4. Toasts (Razer Cortex style)
- Bottom-right of primary monitor, above the taskbar: 392×112 logical px window (card inset 10 px), sized from the primary monitor's scale factor so it is never cut off
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

**Elevation:** many modules need admin (services, HKLM, Prefetch, BAM, uninstall).
- WebView2 can't run elevated, so there are two processes: the **app** (window, tray, pop-ups, widgets, taskbar, tidy — normal user, autostart via HKCU Run) and the **engine** (`--engine`, all system changes + Autopilot) started elevated by the "Nightmoth Engine" scheduled task. First run asks for admin once to create that task.
- App ↔ engine: localhost TCP, random port + token in `%ProgramData%\Nightmoth\engine.json`. The engine checks the owning process of every connection (`GetExtendedTcpTable` → exe path) and only answers its own exe.
- The engine's files live in `%ProgramData%\Nightmoth` with an ACL of Administrators/SYSTEM full, Users read (set at every start). Nothing a normal program can write steers the engine.
- Anything a normal program could have planted (HKCU uninstall strings, relaunching apps after a boost) runs as the user via the app ("user jobs").

**Storage:** engine: `%ProgramData%\Nightmoth\` — `config.json`, `changes.json`, `engine.json`, engine log, generated wallpapers, lock-screen image (migrated once from AppData in 0.3). App: `%APPDATA%\Nightmoth\` — app log, `tidy.json`.

---

## 6. Distribution
- Build in `nightmoth-src` → NSIS installer `Nightmoth-Setup-x.y.z.exe`
- Uploaded to **this repo's GitHub Releases** ("Nightmoth (latest build)" on every push to `main`, versioned on `v*` tags)
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
