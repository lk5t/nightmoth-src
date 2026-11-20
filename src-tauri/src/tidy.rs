//! Tidy desktop (app side, normal user rights): sorts loose files on the desktop into
//! folders by type. Shortcuts and folders stay where they are. The last tidy can be undone.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::win::{self, user_dir, Hive};

const GROUPS: &[(&str, &[&str])] = &[
    ("Images", &["jpg", "jpeg", "png", "gif", "webp", "bmp", "heic", "svg", "psd", "tif", "tiff", "ico", "raw"]),
    ("Videos", &["mp4", "mkv", "mov", "avi", "webm", "wmv", "flv", "m4v"]),
    ("Music", &["mp3", "wav", "flac", "ogg", "m4a", "aac", "wma"]),
    ("Documents", &["pdf", "doc", "docx", "txt", "rtf", "odt", "xls", "xlsx", "csv", "ppt", "pptx", "md", "epub"]),
    ("Archives", &["zip", "rar", "7z", "tar", "gz", "xz", "iso"]),
    ("Installers", &["exe", "msi", "msix", "appx", "bat", "cmd"]),
    ("Code", &["py", "js", "ts", "json", "html", "css", "rs", "cpp", "c", "h", "java", "lua", "xml", "yml", "yaml"]),
];

/// Never moved: shortcuts and system files.
const KEEP_EXT: &[&str] = &["lnk", "url", "ini", "appref-ms"];

#[derive(Serialize, Deserialize, Default)]
struct History {
    last: Vec<(String, String)>,
    ts: i64,
}

fn history_path() -> PathBuf {
    user_dir().join("tidy.json")
}

pub fn desktop() -> PathBuf {
    win::reg_str(Hive::HKCU, r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders", "Desktop")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(win::env("USERPROFILE")).join("Desktop"))
}

fn group_of(p: &Path) -> Option<&'static str> {
    let ext = p.extension()?.to_str()?.to_lowercase();
    if KEEP_EXT.contains(&ext.as_str()) {
        return None;
    }
    // programs (not setup files) are apps: Organize desktop handles those
    if ext == "exe" && crate::organize::is_app_file(p) {
        return None;
    }
    Some(GROUPS.iter().find(|(_, exts)| exts.contains(&ext.as_str())).map(|(g, _)| *g).unwrap_or("Other"))
}

/// Loose files that would be moved: (path, group). `min_age` skips files changed recently.
fn loose(min_age: Duration) -> Vec<(PathBuf, &'static str)> {
    let Ok(rd) = std::fs::read_dir(desktop()) else { return vec![] };
    let now = SystemTime::now();
    rd.filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            let m = std::fs::symlink_metadata(&p).ok()?;
            if !m.is_file() {
                return None;
            }
            let name = p.file_name()?.to_string_lossy().to_string();
            if name.starts_with('.') || name.starts_with('~') {
                return None;
            }
            // skip hidden/system files (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM)
            use std::os::windows::fs::MetadataExt;
            if m.file_attributes() & 0x6 != 0 {
                return None;
            }
            if let Ok(t) = m.modified() {
                if now.duration_since(t).unwrap_or(Duration::ZERO) < min_age {
                    return None;
                }
            }
            group_of(&p).map(|g| (p, g))
        })
        .collect()
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

/// How many files would go into each folder.
pub fn preview() -> serde_json::Value {
    let mut counts: Vec<(String, usize)> = vec![];
    for (_, g) in loose(Duration::ZERO) {
        match counts.iter_mut().find(|(n, _)| n == g) {
            Some(c) => c.1 += 1,
            None => counts.push((g.to_string(), 1)),
        }
    }
    let hist: History = std::fs::read_to_string(history_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    json!({ "groups": counts, "can_undo": !hist.last.is_empty(), "last": hist.ts, "desktop": desktop().to_string_lossy() })
}

/// Sort the desktop. Returns how many files were moved.
pub fn run(min_age: Duration) -> Result<usize, String> {
    let base = desktop();
    let mut moved = vec![];
    for (p, g) in loose(min_age) {
        let dir = base.join(g);
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
        let dest = free_name(&dir, &name);
        if std::fs::rename(&p, &dest).is_ok() {
            moved.push((p.to_string_lossy().to_string(), dest.to_string_lossy().to_string()));
        }
    }
    if !moved.is_empty() {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let h = History { last: moved.clone(), ts };
        let _ = std::fs::write(history_path(), serde_json::to_string(&h).unwrap_or_default());
    }
    Ok(moved.len())
}

/// Put the files from the last tidy back.
pub fn undo() -> Result<usize, String> {
    let hist: History = std::fs::read_to_string(history_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or("Nothing to undo")?;
    let mut n = 0;
    for (from, to) in &hist.last {
        let from = Path::new(from);
        if Path::new(to).exists() && !from.exists() && std::fs::rename(to, from).is_ok() {
            n += 1;
        }
    }
    // remove folders that are empty again
    for g in GROUPS.iter().map(|(g, _)| *g).chain(std::iter::once("Other")) {
        let _ = std::fs::remove_dir(desktop().join(g));
    }
    let _ = std::fs::remove_file(history_path());
    Ok(n)
}

/// Autopilot for the desktop: once a day, if switched on, sort files older than a day.
pub fn start_auto() {
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(10 * 60));
        let Ok(c) = crate::ipc::call_quiet("get_config", json!({})) else { continue };
        if !c["autopilot"].as_bool().unwrap_or(true) || !c["tidy_auto"].as_bool().unwrap_or(false) {
            continue;
        }
        let last = c["tidy_last"].as_i64().unwrap_or(0);
        if win::now() - last < 24 * 3600 {
            continue;
        }
        let n = run(Duration::from_secs(24 * 3600)).unwrap_or(0);
        let _ = crate::ipc::call_quiet("set_config", json!({ "patch": { "tidy_last": win::now() } }));
        if n > 0 {
            crate::ui_toast::local("Desktop tidied", &format!("{n} files sorted into folders · undo from Look"));
        }
    });
}
