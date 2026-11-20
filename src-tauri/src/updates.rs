//! Keeps your apps up to date through winget (Windows' own package manager):
//! checked once a day while you're away and plugged in, installed silently.

use serde::Serialize;

use crate::toast;
use crate::win;

#[derive(Serialize, Clone, Debug)]
pub struct Upgrade {
    pub name: String,
    pub id: String,
    pub version: String,
    pub available: String,
}

const ARGS: &[&str] = &["--accept-source-agreements", "--disable-interactivity"];

/// Apps with a newer version available. Parses winget's table by its header columns.
pub fn available() -> Vec<Upgrade> {
    let mut args = vec!["upgrade"];
    args.extend_from_slice(ARGS);
    let Ok(out) = win::run("winget.exe", &args) else { return vec![] };
    let lines: Vec<&str> = out.lines().map(|l| l.trim_end_matches('\r')).collect();
    let Some(hi) = lines.iter().position(|l| l.contains("Id") && l.contains("Version") && l.contains("Available")) else { return vec![] };
    let header = lines[hi];
    // winget prints a progress spinner before the header; keep only the table part
    let header = header.rsplit('\r').next().unwrap_or(header);
    let col = |name: &str| header.find(name);
    let (Some(c_id), Some(c_ver), Some(c_av)) = (col("Id"), col("Version"), col("Available")) else { return vec![] };
    let c_src = col("Source").unwrap_or(usize::MAX);
    let cut = |l: &str, a: usize, b: usize| -> String {
        let chars: Vec<char> = l.chars().collect();
        chars.get(a.min(chars.len())..b.min(chars.len())).map(|s| s.iter().collect::<String>()).unwrap_or_default().trim().to_string()
    };
    lines[hi + 1..]
        .iter()
        .filter(|l| !l.starts_with('-') && l.chars().count() > c_av)
        .take_while(|l| !l.to_lowercase().contains("upgrades available") && !l.to_lowercase().contains("package(s)"))
        .map(|l| Upgrade { name: cut(l, 0, c_id), id: cut(l, c_id, c_ver), version: cut(l, c_ver, c_av), available: cut(l, c_av, c_src) })
        .filter(|u| !u.id.is_empty() && !u.id.contains(' ') && !u.available.is_empty())
        .collect()
}

/// Update one app (or all with `None`).
pub fn upgrade(id: Option<&str>) -> Result<(), String> {
    let mut args = vec!["upgrade"];
    match id {
        Some(i) => args.extend_from_slice(&["--id", i, "--exact"]),
        None => args.push("--all"),
    }
    args.extend_from_slice(&["--silent", "--accept-package-agreements"]);
    args.extend_from_slice(ARGS);
    win::run("winget.exe", &args).map(|_| ())
}

/// Autopilot: update everything that has an update.
pub fn run_auto() {
    let list = available();
    if list.is_empty() {
        return;
    }
    let mut done = vec![];
    for u in &list {
        if upgrade(Some(&u.id)).is_ok() {
            done.push(u.name.clone());
        }
    }
    if done.is_empty() {
        return;
    }
    let names = if done.len() <= 3 { done.join(", ") } else { format!("{}, {} and {} more", done[0], done[1], done.len() - 2) };
    toast::show("updates", &format!("{} app{} updated", done.len(), if done.len() == 1 { "" } else { "s" }), &names);
    crate::changelog::record("Autopilot", &format!("Updated {} apps", done.len()), &done.join(", "), crate::changelog::Change::Files { bytes: 0 });
}
