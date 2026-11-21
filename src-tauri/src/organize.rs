//! Organize desktop apps (app side): every app shortcut on the desktop goes into a
//! folder for its kind — Games, Social, Browsers, Music & Video, Creative, Work,
//! Dev tools, System & utilities, or Other apps — so nothing is left over.
//!
//! How an app's kind is decided, strongest signal first:
//! 1. your own choice (picking a folder in Nightmoth, or moving it yourself)
//! 2. a big list of known apps (name and exe)
//! 3. where it points: Steam/Epic/Riot/GOG/EA/Ubisoft/Xbox game folders, steam:// links, websites
//! 4. the publisher and description inside the exe
//! 5. the publisher of its uninstall entry
//! 6. otherwise "Other apps"
//!
//! Shortcuts on the shared (Public) desktop are moved by the engine, since they need admin.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::win::{self, user_dir};

pub const KINDS: &[&str] = &["Games", "Social", "Browsers", "Music & Video", "Creative", "Work", "Dev tools", "System & utilities", "Other apps"];

/// (category, keywords matched against the shortcut name and exe file name)
const KNOWN: &[(&str, &[&str])] = &[
    ("Games", &[
        "steam", "epic games", "epicgameslauncher", "riot client", "riotclient", "league of legends", "valorant", "battle.net", "battlenet",
        "ea app", "eadesktop", "origin", "ubisoft", "uplay", "ubisoft connect", "gog galaxy", "galaxyclient", "xbox", "minecraft", "roblox",
        "fortnite", "rocket league", "genshin", "honkai", "zenless", "wuthering", "hoyoplay", "apex", "overwatch", "call of duty", "warzone",
        "counter-strike", "cs2", "dota", "pubg", "gta", "grand theft auto", "red dead", "elden ring", "cyberpunk", "fallout", "skyrim",
        "the sims", "sims 4", "terraria", "factorio", "stardew", "among us", "fall guys", "destiny", "rainbow six", "battlefield", "fifa",
        "ea sports fc", "nba 2k", "forza", "halo", "palworld", "lethal company", "phasmophobia", "osu!", "osu", "geometry dash", "hollow knight",
        "deltarune", "undertale", "warframe", "path of exile", "lost ark", "world of warcraft", "hearthstone", "diablo", "starcraft",
        "marvel rivals", "the finals", "delta force", "war thunder", "world of tanks", "rust", "dayz", "ark", "valheim", "subnautica",
        "satisfactory", "euro truck", "assetto", "iracing", "beamng", "teardown", "rimworld", "civilization", "hearts of iron",
        "crusader kings", "europa universalis", "total war", "baldur's gate", "baldurs gate", "helldivers", "monster hunter", "street fighter",
        "tekken", "mortal kombat", "smite", "paladins", "brawlhalla", "multiversus", "splitgate", "titanfall", "dead by daylight", "rec room",
        "vrchat", "gorilla tag", "beat saber", "itch", "playnite", "heroic games", "amazon games", "prism launcher", "curseforge", "modrinth",
        "lunar client", "badlion", "tlauncher", "bloxstrap", "fishstrap", "geforce now", "boosteroid", "parsec", "moonlight", "retroarch",
        "dolphin", "yuzu", "ryujinx", "pcsx2", "rpcs3", "cemu", "ppsspp", "duckstation", "game",
    ]),
    ("Social", &[
        "discord", "vesktop", "whatsapp", "telegram", "signal", "messenger", "instagram", "facebook", "tiktok", "snapchat", "twitter",
        "x.com", "reddit", "skype", "teamspeak", "mumble", "guilded", "revolt", "element", "slack", "wechat", "line", "viber", "kik",
        "threads", "bluesky", "mastodon", "steam chat", "zoom", "google meet", "facetime", "pinterest", "tumblr", "betterdiscord",
    ]),
    ("Browsers", &[
        "chrome", "google chrome", "firefox", "microsoft edge", "msedge", "brave", "opera", "opera gx", "vivaldi", "arc", "zen browser",
        "tor browser", "librewolf", "waterfox", "chromium", "floorp", "thorium", "yandex", "duckduckgo", "maxthon",
    ]),
    ("Music & Video", &[
        "spotify", "youtube", "youtube music", "netflix", "disney+", "disney plus", "prime video", "amazon music", "apple music", "itunes",
        "tidal", "deezer", "soundcloud", "vlc", "mpv", "mpc-hc", "media player", "potplayer", "plex", "jellyfin", "kodi", "twitch",
        "crunchyroll", "hbo", "max", "hulu", "audacious", "foobar", "musicbee", "winamp", "aimp", "stremio", "kick", "tidal",
    ]),
    ("Creative", &[
        "photoshop", "illustrator", "premiere", "after effects", "lightroom", "indesign", "audition", "adobe", "creative cloud", "gimp",
        "krita", "paint.net", "blender", "davinci", "resolve", "capcut", "obs studio", "obs", "streamlabs", "audacity", "fl studio",
        "ableton", "reaper", "bandlab", "cubase", "pro tools", "figma", "canva", "affinity", "clip studio", "aseprite", "inkscape",
        "shotcut", "kdenlive", "handbrake", "vegas", "filmora", "camtasia", "unity hub", "unity", "unreal", "godot", "maya", "3ds max",
        "cinema 4d", "zbrush", "substance", "sketchup", "fusion 360", "autocad", "medibang", "paint", "snipping", "sharex", "medal",
        "nvidia broadcast", "voicemeeter", "elgato", "stream deck",
    ]),
    ("Work", &[
        "word", "excel", "powerpoint", "outlook", "onenote", "teams", "microsoft 365", "office", "libreoffice", "openoffice", "onlyoffice",
        "wps", "notion", "obsidian", "evernote", "todoist", "trello", "asana", "clickup", "google docs", "google drive", "onedrive",
        "dropbox", "box", "mega", "acrobat", "pdf", "foxit", "sumatra", "calibre", "kindle", "zotero", "anki", "grammarly", "deepl",
        "chatgpt", "claude", "copilot", "perplexity", "gemini", "thunderbird", "mailbird", "calendar", "1password", "bitwarden",
        "lastpass", "keepass", "dashlane", "proton", "nordpass",
    ]),
    ("Dev tools", &[
        "visual studio", "vs code", "code", "cursor", "intellij", "pycharm", "webstorm", "rider", "clion", "goland", "android studio",
        "jetbrains", "sublime", "notepad++", "atom", "vim", "neovim", "emacs", "git", "github desktop", "gitkraken", "sourcetree", "docker",
        "postman", "insomnia", "wsl", "ubuntu", "terminal", "powershell", "putty", "winscp", "filezilla", "node", "python", "anaconda",
        "dbeaver", "mysql", "pgadmin", "mongodb", "heidisql", "wireshark", "fiddler", "virtualbox", "vmware", "hyper-v", "arduino",
        "eclipse", "netbeans", "xcode", "zed", "windsurf",
    ]),
    ("System & utilities", &[
        "control panel", "settings", "task manager", "file explorer", "this pc", "recycle bin", "nvidia", "geforce", "amd software",
        "radeon", "adrenalin", "intel", "msi afterburner", "rivatuner", "hwinfo", "hwmonitor", "cpu-z", "gpu-z", "crystaldisk", "speccy",
        "ccleaner", "malwarebytes", "defender", "windows security", "avast", "avg", "norton", "mcafee", "kaspersky", "bitdefender", "eset",
        "7-zip", "winrar", "peazip", "bandizip", "everything", "powertoys", "autohotkey", "rufus", "balena", "etcher", "ventoy",
        "teamviewer", "anydesk", "rustdesk", "parsec", "nordvpn", "expressvpn", "protonvpn", "surfshark", "mullvad", "windscribe",
        "cloudflare warp", "qbittorrent", "utorrent", "bittorrent", "deluge", "transmission", "jdownloader", "idm", "internet download",
        "logitech", "g hub", "razer", "synapse", "corsair", "icue", "steelseries", "wootility", "armoury crate", "msi center",
        "lenovo vantage", "dell", "hp ", "asus", "nightmoth", "rainmeter", "wallpaper engine", "lively", "translucenttb", "windhawk",
        "startallback", "explorerpatcher", "open-shell", "fences", "revo uninstaller", "geek uninstaller", "bulk crap", "treesize",
        "windirstat", "wiztree", "spacesniffer", "sysinternals", "process explorer", "autoruns", "regedit", "cmd", "command prompt",
        "disk cleanup", "device manager", "remote desktop", "uninstall", "updater", "driver",
    ]),
];

/// Folders that mean "this is a game" when a shortcut points inside them.
const GAME_PATHS: &[&str] = &[
    "\\steamapps\\common\\", "\\epic games\\", "\\riot games\\", "\\gog galaxy\\games\\", "\\gog games\\", "\\ea games\\",
    "\\electronic arts\\", "\\ubisoft game launcher\\games\\", "\\ubisoft\\", "\\xboxgames\\", "\\battle.net\\", "\\blizzard",
    "\\roblox\\", "\\minecraft", "\\hoyoplay\\", "\\genshin impact\\", "\\games\\", "\\itch\\apps\\", "\\amazon games\\",
];

/// Publisher → category, for exes whose name says nothing.
const PUBLISHERS: &[(&str, &str)] = &[
    ("valve", "Games"), ("epic games", "Games"), ("riot games", "Games"), ("blizzard", "Games"), ("electronic arts", "Games"),
    ("ubisoft", "Games"), ("rockstar", "Games"), ("bethesda", "Games"), ("cd projekt", "Games"), ("mojang", "Games"),
    ("roblox", "Games"), ("cognosphere", "Games"), ("mihoyo", "Games"), ("hoyoverse", "Games"), ("bandai namco", "Games"),
    ("square enix", "Games"), ("capcom", "Games"), ("sega", "Games"), ("2k", "Games"), ("activision", "Games"), ("krafton", "Games"),
    ("unity technologies", "Creative"), ("adobe", "Creative"), ("blackmagic", "Creative"), ("autodesk", "Creative"), ("serif", "Creative"),
    ("image-line", "Creative"), ("obs project", "Creative"), ("jetbrains", "Dev tools"), ("github", "Dev tools"), ("docker", "Dev tools"),
    ("postman", "Dev tools"), ("python software", "Dev tools"), ("discord", "Social"), ("telegram", "Social"), ("whatsapp", "Social"),
    ("signal messenger", "Social"), ("slack", "Social"), ("zoom", "Social"), ("spotify", "Music & Video"), ("videolan", "Music & Video"),
    ("netflix", "Music & Video"), ("plex", "Music & Video"), ("google", "Browsers"), ("mozilla", "Browsers"), ("brave software", "Browsers"),
    ("opera", "Browsers"), ("vivaldi", "Browsers"), ("notion", "Work"), ("dropbox", "Work"), ("nvidia", "System & utilities"),
    ("advanced micro devices", "System & utilities"), ("intel", "System & utilities"), ("logitech", "System & utilities"),
    ("razer", "System & utilities"), ("corsair", "System & utilities"), ("malwarebytes", "System & utilities"),
];

/// Website → category, for internet shortcuts.
const SITES: &[(&str, &str)] = &[
    ("youtube.", "Music & Video"), ("netflix.", "Music & Video"), ("twitch.", "Music & Video"), ("spotify.", "Music & Video"),
    ("crunchyroll.", "Music & Video"), ("disneyplus.", "Music & Video"), ("primevideo.", "Music & Video"), ("discord.", "Social"),
    ("reddit.", "Social"), ("instagram.", "Social"), ("facebook.", "Social"), ("x.com", "Social"), ("twitter.", "Social"),
    ("tiktok.", "Social"), ("whatsapp.", "Social"), ("roblox.", "Games"), ("store.steampowered", "Games"), ("epicgames.", "Games"),
    ("itch.io", "Games"), ("github.", "Dev tools"), ("stackoverflow.", "Dev tools"), ("docs.google", "Work"), ("notion.", "Work"),
    ("chatgpt.", "Work"), ("claude.ai", "Work"), ("figma.", "Creative"), ("canva.", "Creative"),
];

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Memory {
    /// file name -> category Nightmoth put it in
    placed: HashMap<String, String>,
    /// app name (lowercase, no extension) -> category you chose
    overrides: HashMap<String, String>,
    /// last organize: (from, to, from public desktop)
    last: Vec<(String, String, bool)>,
    ts: i64,
    /// apps placed by the last arrange (full paths, lowercase)
    arranged: Vec<String>,
    /// icon positions before the last arrange, for Undo: (parsing name, x, y)
    before: Vec<(String, i32, i32)>,
}

fn memory_path() -> PathBuf {
    user_dir().join("organize.json")
}

fn load() -> Memory {
    std::fs::read_to_string(memory_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(m: &Memory) {
    let _ = std::fs::write(memory_path(), serde_json::to_string(m).unwrap_or_default());
}

pub fn public_desktop() -> PathBuf {
    let p = win::env("PUBLIC");
    PathBuf::from(if p.is_empty() { r"C:\Users\Public".to_string() } else { p }).join("Desktop")
}

fn app_key(file: &Path) -> String {
    file.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn is_app_file(p: &Path) -> bool {
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "lnk" | "url" | "appref-ms" => true,
        // a bare program on the desktop is an app too — unless it's clearly a setup file (Tidy handles those)
        "exe" => {
            let n = app_key(p);
            !["setup", "install", "installer", "update", "uninst", "-x64", "_x64", "win64", "redist"].iter().any(|k| n.contains(k))
        }
        _ => false,
    }
}

// ------------------------------------------------------------------ reading shortcuts

/// (target path, arguments) of a .lnk, or the URL of a .url.
pub fn target_of(p: &Path) -> (String, String) {
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if ext == "url" {
        let s = std::fs::read_to_string(p).unwrap_or_default();
        let url = s.lines().find_map(|l| l.trim().strip_prefix("URL=")).unwrap_or("").to_string();
        return (url, String::new());
    }
    if ext == "exe" {
        return (p.to_string_lossy().to_string(), String::new());
    }
    if ext != "lnk" {
        return (String::new(), String::new());
    }
    use windows::core::{Interface, HSTRING, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { return Default::default() };
        let Ok(pf) = link.cast::<IPersistFile>() else { return Default::default() };
        let h = HSTRING::from(p.as_os_str());
        if pf.Load(PCWSTR(h.as_ptr()), STGM_READ).is_err() {
            return Default::default();
        }
        let mut buf = [0u16; 1024];
        let _ = link.GetPath(&mut buf, std::ptr::null_mut(), 0);
        let target = String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(0)]);
        let mut args = [0u16; 1024];
        let _ = link.GetArguments(&mut args);
        let args = String::from_utf16_lossy(&args[..args.iter().position(|c| *c == 0).unwrap_or(0)]);
        (target, args)
    }
}

/// CompanyName and FileDescription from an exe's version info.
fn version_strings(exe: &str) -> (String, String) {
    use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let w = win::wide(exe);
    unsafe {
        let size = GetFileVersionInfoSizeW(w.as_ptr(), std::ptr::null_mut());
        if size == 0 || size > 4 * 1024 * 1024 {
            return Default::default();
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(w.as_ptr(), 0, size, data.as_mut_ptr() as *mut _) == 0 {
            return Default::default();
        }
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let tr = win::wide("\\VarFileInfo\\Translation");
        let lang = if VerQueryValueW(data.as_ptr() as *const _, tr.as_ptr(), &mut ptr, &mut len) != 0 && len >= 4 {
            let p = ptr as *const u16;
            format!("{:04x}{:04x}", *p, *p.add(1))
        } else {
            "040904b0".to_string()
        };
        let get = |key: &str| -> String {
            let q = win::wide(&format!("\\StringFileInfo\\{lang}\\{key}"));
            let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut l = 0u32;
            if VerQueryValueW(data.as_ptr() as *const _, q.as_ptr(), &mut p, &mut l) != 0 && l > 0 {
                let s = std::slice::from_raw_parts(p as *const u16, l as usize);
                String::from_utf16_lossy(&s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())])
            } else {
                String::new()
            }
        };
        (get("CompanyName"), get("FileDescription"))
    }
}

fn keyword_kind(text: &str) -> Option<&'static str> {
    let t = format!(" {} ", text.to_lowercase().replace(['_', '-', '.'], " "));
    // longest keyword wins, so "steam chat" beats "steam" and "vs code" beats "code"
    let mut best: Option<(&str, usize)> = None;
    for (kind, words) in KNOWN {
        for w in *words {
            let w2 = format!(" {} ", w.replace(['_', '-', '.'], " "));
            if t.contains(&w2) && best.map(|b| w.len() > b.1).unwrap_or(true) {
                best = Some((kind, w.len()));
            }
        }
    }
    best.map(|b| b.0)
}

/// The kind of app a desktop item is, and why (shown in the UI).
pub fn classify(p: &Path, uninstall: &[(String, String)]) -> (&'static str, String) {
    let name = app_key(p);
    let (target, args) = target_of(p);
    let tl = target.to_lowercase();
    let exe_name = Path::new(&target).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();

    if tl.starts_with("steam://") || tl.starts_with("com.epicgames.launcher://") || tl.starts_with("roblox") || tl.starts_with("uplay://")
        || tl.starts_with("origin://") || tl.starts_with("battlenet://") || tl.starts_with("riotclient") || args.contains("--launch-product")
        || args.to_lowercase().contains("rungameid")
    {
        return ("Games", "game launcher link".into());
    }
    if GAME_PATHS.iter().any(|g| tl.contains(g)) {
        return ("Games", "installed in a game library".into());
    }
    if let Some(k) = keyword_kind(&name).or_else(|| keyword_kind(&exe_name)) {
        return (k, "known app".into());
    }
    if tl.starts_with("http") {
        if let Some((_, k)) = SITES.iter().find(|(s, _)| tl.contains(s)) {
            return (k, "website".into());
        }
        return ("Browsers", "website".into());
    }
    if tl.ends_with(".exe") {
        let (company, desc) = version_strings(&target);
        let cl = company.to_lowercase();
        if let Some((_, k)) = PUBLISHERS.iter().find(|(p, _)| cl.contains(p)) {
            return (k, format!("made by {company}"));
        }
        if let Some(k) = keyword_kind(&desc) {
            return (k, format!("\"{desc}\""));
        }
        // engines used by games
        let dir = Path::new(&target).parent().map(|d| d.to_path_buf()).unwrap_or_default();
        if dir.join("UnityPlayer.dll").exists() || dir.join("Engine").is_dir() || dir.join("steam_api64.dll").exists() || dir.join("steam_api.dll").exists() {
            return ("Games", "game engine files next to it".into());
        }
        if let Some((_, publisher)) = uninstall.iter().find(|(loc, _)| !loc.is_empty() && tl.starts_with(loc.as_str())) {
            let pl = publisher.to_lowercase();
            if let Some((_, k)) = PUBLISHERS.iter().find(|(p, _)| pl.contains(p)) {
                return (k, format!("made by {publisher}"));
            }
        }
        if tl.contains("\\windows\\") || tl.contains("\\system32\\") {
            return ("System & utilities", "part of Windows".into());
        }
    }
    ("Other apps", "no clear match — move it if it belongs elsewhere".into())
}

/// Install folders and publishers of installed apps (lowercase), for the last-resort match.
fn uninstall_index() -> Vec<(String, String)> {
    let mut out = vec![];
    for (hive, base) in [
        (win::Hive::HKLM, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (win::Hive::HKLM, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (win::Hive::HKCU, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
    ] {
        for sub in win::reg_subkeys(hive, base) {
            let path = format!(r"{base}\{sub}");
            let loc = win::reg_str(hive, &path, "InstallLocation").unwrap_or_default().trim_matches('"').trim_end_matches('\\').to_lowercase();
            let publisher = win::reg_str(hive, &path, "Publisher").unwrap_or_default();
            if loc.len() > 3 && !publisher.is_empty() {
                out.push((loc, publisher));
            }
        }
    }
    out
}

// ------------------------------------------------------------------ organizing

#[derive(Serialize, Clone)]
pub struct DesktopApp {
    pub name: String,
    pub file: String,
    pub kind: String,
    pub why: String,
    pub public: bool,
    /// already placed with its group
    pub sorted: bool,
}

fn older_than(p: &Path, secs: u64) -> bool {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .map(|t| SystemTime::now().duration_since(t).unwrap_or(Duration::ZERO) > Duration::from_secs(secs))
        .unwrap_or(true)
}

fn free_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = p.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    (2..1000).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|c| !c.exists()).unwrap_or(first)
}

/// Before 0.8 apps were put into category folders: take them back out onto the desktop.
fn flatten_folders(mem: &mut Memory) {
    let base = crate::tidy::desktop();
    let mut public_back = vec![];
    for kind in KINDS {
        let dir = base.join(kind);
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            let from_public = mem.last.iter().find(|l| l.2 && Path::new(&l.1) == p);
            if let Some((orig, _, _)) = from_public {
                let name = Path::new(orig).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                public_back.push(json!({ "from": p.to_string_lossy(), "name": name }));
                continue;
            }
            let dest = free_name(&base, &e.file_name().to_string_lossy());
            let _ = std::fs::rename(&p, &dest);
        }
        if !public_back.is_empty() {
            let _ = crate::ipc::call("public_desktop_restore", json!({ "moves": public_back.clone() }));
            public_back.clear();
        }
        let _ = std::fs::remove_dir(&dir);
    }
    mem.last.clear();
}

/// Everything on the desktop that's an app, and the group it belongs to.
pub fn scan() -> Vec<DesktopApp> {
    let mem = load();
    let base = crate::tidy::desktop();
    let index = uninstall_index();
    let mut out = vec![];
    for (dir, public) in [(base.clone(), false), (public_desktop(), true)] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if !e.file_type().map(|t| t.is_file()).unwrap_or(false) || !is_app_file(&p) {
                continue;
            }
            let key = app_key(&p);
            let (kind, why) = match mem.overrides.get(&key) {
                Some(k) => (k.clone(), "your choice".to_string()),
                None => {
                    let (k, w) = classify(&p, &index);
                    (k.to_string(), w)
                }
            };
            let file = p.to_string_lossy().to_string();
            out.push(DesktopApp {
                name: p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                sorted: mem.arranged.contains(&file.to_lowercase()),
                file,
                kind,
                why,
                public,
            });
        }
    }
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

// ------------------------------------------------------------------ the desktop's own icon view

use windows::core::Interface;
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, IServiceProvider, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    IFolderView2, IShellBrowser, IShellWindows, ShellWindows, FWF_AUTOARRANGE, SID_STopLevelBrowser, SIGDN_DESKTOPABSOLUTEPARSING, SVGIO_ALLVIEW,
    SVSI_POSITIONITEM, SWC_DESKTOP, SWFO_NEEDDISPATCH,
};

struct Icon {
    name: String,
    pidl: *mut ITEMIDLIST,
    pos: POINT,
}

struct View {
    fv: IFolderView2,
    icons: Vec<Icon>,
}

impl Drop for View {
    fn drop(&mut self) {
        for i in &self.icons {
            unsafe { CoTaskMemFree(Some(i.pidl as *const _)) };
        }
    }
}

/// The desktop's icon view (the same one Windows uses when you drag icons around).
fn desktop_view() -> Result<View, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let sw: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let mut hwnd = 0i32;
        let empty = VARIANT::default();
        let disp = sw.FindWindowSW(&empty, &empty, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH).map_err(|e| e.to_string())?;
        let sp: IServiceProvider = disp.cast().map_err(|e| e.to_string())?;
        let browser: IShellBrowser = sp.QueryService(&SID_STopLevelBrowser).map_err(|e| e.to_string())?;
        let view = browser.QueryActiveShellView().map_err(|e| e.to_string())?;
        let fv: IFolderView2 = view.cast().map_err(|e| e.to_string())?;
        let n = fv.ItemCount(SVGIO_ALLVIEW).map_err(|e| e.to_string())?;
        let mut icons = vec![];
        for i in 0..n {
            let Ok(item) = fv.GetItem::<windows::Win32::UI::Shell::IShellItem>(i) else { continue };
            let Ok(namep) = item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING) else { continue };
            let name = namep.to_string().unwrap_or_default();
            CoTaskMemFree(Some(namep.0 as *const _));
            let Ok(pidl) = fv.Item(i) else { continue };
            let pos = fv.GetItemPosition(pidl).unwrap_or_default();
            icons.push(Icon { name, pidl, pos });
        }
        Ok(View { fv, icons })
    }
}

fn place(view: &View, moves: &[(usize, POINT)]) -> Result<(), String> {
    if moves.is_empty() {
        return Ok(());
    }
    unsafe {
        // Windows' "Auto arrange icons" would undo every placement
        let _ = view.fv.SetCurrentFolderFlags(FWF_AUTOARRANGE.0 as u32, 0);
        let pidls: Vec<*const ITEMIDLIST> = moves.iter().map(|(i, _)| view.icons[*i].pidl as *const _).collect();
        let pts: Vec<POINT> = moves.iter().map(|m| m.1).collect();
        view.fv.SelectAndPositionItems(pidls.len() as u32, pidls.as_ptr(), Some(pts.as_ptr()), SVSI_POSITIONITEM.0 as u32).map_err(|e| e.to_string())
    }
}

/// Which side of the screen the groups go on, and how the wallpaper looks there.
fn layout_side() -> bool {
    // true = left
    let Ok(h) = crate::ipc::call_quiet("desktop_layout_hint", json!({})) else { return true };
    let busy: Vec<f64> = h["busy"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default();
    if busy.len() != 9 {
        return true;
    }
    let mut left = busy[0] + busy[3] + busy[6];
    let mut right = busy[2] + busy[5] + busy[8];
    // keep clear of the widgets
    if h["widgets_on"].as_bool().unwrap_or(false) {
        match h["widget_cell"].as_u64().unwrap_or(2) % 3 {
            0 => left += 2.0,
            2 => right += 2.0,
            _ => {}
        }
    }
    left <= right * 1.15
}

/// Lay every desktop icon out in tidy groups: each kind is a compact block, blocks run down
/// the calmer side of the wallpaper (away from the widgets) with a gap between them.
fn arrange(apps: &[DesktopApp]) -> Result<usize, String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};
    let view = desktop_view()?;
    if view.icons.is_empty() {
        return Ok(0);
    }
    let mut spacing = POINT::default();
    unsafe {
        let _ = view.fv.GetSpacing(&mut spacing);
    }
    let (cx, cy) = (spacing.x.max(60), spacing.y.max(70));
    let (l, t, r, b) = crate::win::work_area();
    let (vx, vy) = unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN)) };
    let (ox, oy) = (l - vx + cx / 8, t - vy + cy / 10);
    let cols = ((r - l) / cx).max(1);
    let rows = ((b - t) / cy).max(1);

    // groups: Windows' own icons first (Recycle Bin, This PC), then each kind, then files & folders
    let mut groups: Vec<Vec<usize>> = vec![vec![]; KINDS.len() + 2];
    for (i, ic) in view.icons.iter().enumerate() {
        let lname = ic.name.to_lowercase();
        if lname.starts_with("::") {
            groups[0].push(i);
        } else if let Some(a) = apps.iter().find(|a| a.file.to_lowercase() == lname) {
            let k = KINDS.iter().position(|k| *k == a.kind).unwrap_or(KINDS.len() - 1);
            groups[k + 1].push(i);
        } else {
            groups[KINDS.len() + 1].push(i);
        }
    }
    for g in groups.iter_mut() {
        g.sort_by_key(|i| {
            let n = &view.icons[*i].name;
            Path::new(n).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_else(|| n.to_lowercase())
        });
    }

    let left = layout_side();
    let mut moves: Vec<(usize, POINT)> = vec![];
    let (mut band, mut band_w, mut row) = (0i32, 0i32, 0i32);
    for g in groups.iter().filter(|g| !g.is_empty()) {
        let n = g.len() as i32;
        // compact blocks: up to 5 tall, a bit taller for big groups
        let mut h = if n <= 10 { n.min(5) } else { (n + (n + 5) / 6 - 1) / ((n + 5) / 6) };
        h = h.min(rows);
        let w = (n + h - 1) / h;
        if row > 0 && row + h > rows {
            band += band_w;
            band_w = 0;
            row = 0;
        }
        for (j, idx) in g.iter().enumerate() {
            let j = j as i32;
            let col = band + j / h;
            let col = if left { col } else { cols - 1 - col };
            let rr = row + j % h;
            moves.push((*idx, POINT { x: ox + col.clamp(0, cols - 1) * cx, y: oy + rr.min(rows - 1) * cy }));
        }
        band_w = band_w.max(w);
        row += h + 1; // one empty space between groups
        if row >= rows {
            band += band_w;
            band_w = 0;
            row = 0;
        }
    }
    // remember where everything was, for Undo
    let mut mem = load();
    mem.before = view.icons.iter().map(|i| (i.name.clone(), i.pos.x, i.pos.y)).collect();
    save(&mem);
    place(&view, &moves)?;
    Ok(moves.len())
}

/// Group every app with similar ones on the desktop. `min_age` waits for brand-new shortcuts
/// (an installer may still be adding more).
pub fn run(min_age: Duration) -> Result<Vec<(String, String)>, String> {
    let mut mem = load();
    if mem.placed.len() + mem.last.len() > 0 || KINDS.iter().any(|k| crate::tidy::desktop().join(k).is_dir()) {
        flatten_folders(&mut mem);
        mem.placed.clear();
        save(&mem);
    }
    let apps = scan();
    if min_age > Duration::ZERO && apps.iter().any(|a| !older_than(Path::new(&a.file), min_age.as_secs())) {
        return Ok(vec![]);
    }
    let fresh: Vec<(String, String)> = apps.iter().filter(|a| !a.sorted).map(|a| (a.name.clone(), a.kind.clone())).collect();
    arrange(&apps)?;
    let mut mem = load();
    mem.arranged = apps.iter().map(|a| a.file.to_lowercase()).collect();
    mem.ts = win::now();
    save(&mem);
    Ok(fresh)
}

/// Pick a group for an app yourself (remembered from now on), and re-arrange.
pub fn set_kind(file: &str, kind: &str) -> Result<(), String> {
    if !KINDS.contains(&kind) {
        return Err("Unknown group".into());
    }
    let mut mem = load();
    mem.overrides.insert(app_key(Path::new(file)), kind.into());
    save(&mem);
    run(Duration::ZERO).map(|_| ())
}

/// Put every icon back where it was before the last arrange.
pub fn undo() -> Result<usize, String> {
    let mem = load();
    if mem.before.is_empty() {
        return Err("Nothing to undo".into());
    }
    let view = desktop_view()?;
    let moves: Vec<(usize, POINT)> = view
        .icons
        .iter()
        .enumerate()
        .filter_map(|(i, ic)| mem.before.iter().find(|b| b.0 == ic.name).map(|b| (i, POINT { x: b.1, y: b.2 })))
        .collect();
    place(&view, &moves)?;
    let mut mem = load();
    mem.before.clear();
    mem.arranged.clear();
    save(&mem);
    Ok(moves.len())
}

pub fn preview() -> Value {
    let apps = scan();
    let mem = load();
    json!({ "apps": apps, "kinds": KINDS, "can_undo": !mem.before.is_empty(), "last": mem.ts })
}

/// Autopilot: when a new app lands on the desktop, it joins its group within a minute.
pub fn start_auto() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(30));
        let Ok(c) = crate::ipc::call_quiet("get_config", json!({})) else { continue };
        if !c["autopilot"].as_bool().unwrap_or(true) || !c["organize_auto"].as_bool().unwrap_or(false) {
            continue;
        }
        let pending: Vec<DesktopApp> = scan().into_iter().filter(|a| !a.sorted).collect();
        if pending.is_empty() {
            continue;
        }
        if let Ok(moved) = run(Duration::from_secs(45)) {
            if !moved.is_empty() {
                let body = if moved.len() == 1 {
                    format!("{} placed with your {}", moved[0].0, moved[0].1.to_lowercase())
                } else {
                    format!("{} apps placed with similar ones", moved.len())
                };
                crate::ui_toast::local("Desktop organized", &body);
            }
        }
    });
}

// ------------------------------------------------------------------ engine side: the shared desktop

fn inside_kind_folder(p: &Path) -> bool {
    let base = crate::tidy::desktop();
    let Some(parent) = p.parent() else { return false };
    KINDS.iter().any(|k| parent == base.join(k)) && std::fs::symlink_metadata(parent).map(|m| m.is_dir() && !m.file_type().is_symlink()).unwrap_or(false)
}

fn plain_shortcut_name(n: &str) -> bool {
    let l = n.to_lowercase();
    !n.is_empty() && !n.contains(['\\', '/', ':']) && !n.contains("..") && (l.ends_with(".lnk") || l.ends_with(".url"))
}

fn move_file(from: &Path, to: &Path) -> bool {
    std::fs::rename(from, to).is_ok() || (std::fs::copy(from, to).is_ok() && std::fs::remove_file(from).is_ok())
}

/// Engine: move shortcuts from the shared desktop into your own desktop folders.
pub fn public_move(moves: &[Value]) -> usize {
    let mut n = 0;
    for m in moves {
        let (Some(name), Some(dest)) = (m["name"].as_str(), m["dest"].as_str()) else { continue };
        let src = public_desktop().join(name);
        let dest = PathBuf::from(dest);
        if plain_shortcut_name(name) && src.is_file() && inside_kind_folder(&dest) && !dest.exists() && move_file(&src, &dest) {
            n += 1;
        }
    }
    n
}

/// Engine: put them back on the shared desktop.
pub fn public_restore(moves: &[Value]) -> usize {
    let mut n = 0;
    for m in moves {
        let (Some(from), Some(name)) = (m["from"].as_str(), m["name"].as_str()) else { continue };
        let from = PathBuf::from(from);
        let dest = public_desktop().join(name);
        if plain_shortcut_name(name) && from.is_file() && inside_kind_folder(&from) && !dest.exists() && move_file(&from, &dest) {
            n += 1;
        }
    }
    n
}
