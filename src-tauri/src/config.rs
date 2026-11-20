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
