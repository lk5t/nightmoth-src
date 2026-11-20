use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::win::{data_dir, now};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Look {
    pub accent: String,
    pub dark: bool,
    pub palette: Vec<String>,
    pub style: String,
    /// Index into the palette chosen with Shuffle (None = automatic pick).
    pub accent_index: Option<usize>,
    /// Calmest 3x3 cell for desktop widgets (filled in on first use).
    pub calm_cell: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct GameCfg {
    pub name: String,
    pub exe_path: String,
    pub boost: bool,
    pub plays: u32,
    pub last_played: i64,
    pub minutes: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct RemovedApp {
    pub name: String,
    pub date: i64,
    pub size_mb: u64,
    pub days_unused: i64,
    pub reinstalled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Stats {
    pub week_start: i64,
    pub freed_bytes: u64,
    pub apps_removed: u32,
    pub games_boosted: u32,
    pub startup_blocked: u32,
    pub cleanups: u32,
}

/// Saved while a game boost is active so it can be undone after a crash.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct BoostSession {
    pub game: String,
    pub exe: String,
    pub pid: u32,
    pub started: i64,
    pub prev_plan: String,
    pub prev_toasts: Option<u32>,
    pub closed: Vec<String>,
    /// Delivery Optimization bandwidth policy values before the boost
    pub prev_do: Vec<(String, Option<u32>)>,
}

/// A startup app Nightmoth starts 60 s after login instead of at login.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct DelayedApp {
    pub id: String,
    pub name: String,
    pub command: String,
}

/// One day's measurements for the Progress page.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Snapshot {
    pub t: i64,
    pub boot_ms: i64,
    pub idle_cpu: f32,
    pub idle_ram_mb: u64,
    pub startup_apps: u32,
    pub disk_free: u64,
    pub junk_bytes: u64,
    pub apps: u32,
    pub score: u32,
}

/// A problem left behind by another optimizer tool, and what Nightmoth did about it.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct FixRecord {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub t: i64,
    pub action: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub first_run_done: bool,
    pub autostart: bool,
    pub autopilot: bool,

    // look
    pub auto_theme: bool,
    pub current_wallpaper: String,
    pub lock_accent: Option<String>,
    pub looks: HashMap<String, Look>,
    pub lock_screen_sync: bool,
    pub slideshow_on: bool,
    pub slideshow_folder: String,
    pub slideshow_minutes: u32,
    pub slideshow_last: i64,
    pub slideshow_index: usize,

    // autopilot
    pub unused_cleaner: bool,
    pub unused_days: u32,
    pub last_unused_check: i64,
    pub auto_cleanup: bool,
    pub cleanup_every_days: u32,
    pub last_cleanup: i64,
    pub startup_watcher: bool,
    pub hog_watcher: bool,
    pub hog_cpu: f32,
    pub hog_ram_mb: u64,
    pub hog_auto_lower: bool,

    // games
    pub game_booster: bool,
    pub boost_close: Vec<String>,
    pub games: HashMap<String, GameCfg>,
    pub pending_boost: Option<BoostSession>,
    pub ultimate_guid: String,

    // toasts (always on; only the duration can change)
    pub toast_ms: u64,

    // desktop widgets
    pub widgets_on: bool,
    /// Which widgets, in order: clock, calendar, system, week
    pub widgets: Vec<String>,
    /// "auto" = the calmest part of the wallpaper, or one of the 9 spots (top left … bottom right)
    pub widget_spot: String,
    /// "glass" | "solid" | "minimal"
    pub widget_style: String,
    /// Colour widgets with the wallpaper's accent (off = Nightmoth red)
    pub widget_accent: bool,
    pub widget_scale: f32,

    /// Taskbar background: "clear" (invisible, only icons show) | "tint" (accent glass) | "normal"
    pub taskbar_style: String,
    /// 0.3 replaced auto-hide with the see-through taskbar (done once)
    pub taskbar_migrated: bool,

    // desktop tidy
    pub tidy_auto: bool,
    pub tidy_last: i64,

    // widgets (extra)
    pub notes: String,
    pub weather_city: String,
    pub weather_lat: f64,
    pub weather_lon: f64,
    /// exe / shortcut paths for the dock widget
    pub dock: Vec<String>,

    // more looks
    pub theme_apps: bool,
    pub accent_cursor: bool,
    pub accent_folders: bool,
    /// monitor device path -> wallpaper (empty = same everywhere)
    pub monitor_walls: HashMap<String, String>,
    pub taskbar_helper: bool,
    pub taskbar_helper_crashes: u32,

    // run-when-needed upkeep
    pub last_deep_clean: i64,
    pub last_junk_check: i64,
    pub last_fixer: i64,
    pub last_update_check: i64,
    pub last_snapshot: i64,
    pub auto_updates: bool,
    pub deep_clean: bool,
    pub fix_other_tools: bool,
    pub fixes: Vec<FixRecord>,
    pub battery_mode: bool,
    pub battery_prev_plan: String,
    pub wu_control: bool,
    pub visual_auto: bool,
    pub visual_done: bool,
    pub delayed: Vec<DelayedApp>,
    pub roblox_tune: bool,
    pub history: Vec<Snapshot>,
    pub dns_auto: bool,
    pub dns_last: i64,
    /// every app Nightmoth has seen installed (name|publisher), to spot leftovers after uninstalls
    pub seen_apps: Vec<String>,
    pub last_deep_report: Vec<(String, u64)>,

    // lists
    pub keep_list: Vec<String>,
    pub startup_allow: Vec<String>,
    pub known_startup: Vec<String>,
    pub startup_seeded: bool,
    pub removed_apps: Vec<RemovedApp>,

    // stats
    pub stats: Stats,
    pub last_week: Stats,
    pub last_report: i64,
    pub last_restore_point: i64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            first_run_done: false,
            autostart: true,
            autopilot: true,
            auto_theme: true,
            current_wallpaper: String::new(),
            lock_accent: None,
            looks: HashMap::new(),
            lock_screen_sync: true,
            slideshow_on: false,
            slideshow_folder: String::new(),
            slideshow_minutes: 60,
            slideshow_last: 0,
            slideshow_index: 0,
            unused_cleaner: true,
            unused_days: 90,
            last_unused_check: 0,
            auto_cleanup: true,
            cleanup_every_days: 7,
            last_cleanup: 0,
            startup_watcher: true,
            hog_watcher: true,
            hog_cpu: 25.0,
            hog_ram_mb: 1536,
            hog_auto_lower: true,
            game_booster: true,
            boost_close: vec![],
            games: HashMap::new(),
            pending_boost: None,
            ultimate_guid: String::new(),
            toast_ms: 5000,
            widgets_on: true,
            widgets: vec!["clock".into(), "calendar".into(), "system".into()],
            widget_spot: "auto".into(),
            widget_style: "glass".into(),
            widget_accent: true,
            widget_scale: 1.0,
            taskbar_style: "clear".into(),
            taskbar_migrated: false,
            tidy_auto: true,
            tidy_last: 0,
            notes: String::new(),
            weather_city: String::new(),
            weather_lat: 0.0,
            weather_lon: 0.0,
            dock: vec![],
            theme_apps: true,
            accent_cursor: false,
            accent_folders: false,
            monitor_walls: HashMap::new(),
            taskbar_helper: true,
            taskbar_helper_crashes: 0,
            last_deep_clean: 0,
            last_junk_check: 0,
            last_fixer: 0,
            last_update_check: 0,
            last_snapshot: 0,
            auto_updates: true,
            deep_clean: true,
            fix_other_tools: true,
            fixes: vec![],
            battery_mode: true,
            battery_prev_plan: String::new(),
            wu_control: true,
            visual_auto: true,
            visual_done: false,
            delayed: vec![],
            roblox_tune: true,
            history: vec![],
            dns_auto: false,
            dns_last: 0,
            seen_apps: vec![],
            last_deep_report: vec![],
            keep_list: vec![],
            startup_allow: vec![],
            known_startup: vec![],
            startup_seeded: false,
            removed_apps: vec![],
            stats: Stats { week_start: now(), ..Default::default() },
            last_week: Stats::default(),
            last_report: now(),
            last_restore_point: 0,
        }
    }
}

impl Config {
    pub fn load() -> Config {
        crate::win::read_state_file("config.json")
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let p = data_dir().join("config.json");
        let tmp = data_dir().join("config.json.tmp");
        if let Ok(s) = serde_json::to_string_pretty(self) {
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }

    pub fn is_kept(&self, key: &str) -> bool {
        let k = key.to_lowercase();
        self.keep_list.iter().any(|x| x.to_lowercase() == k)
    }
}
