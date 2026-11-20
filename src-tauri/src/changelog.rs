//! Every change Nightmoth makes is recorded here so it can be undone.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::state::{core, lock};
use crate::win::{self, data_dir, now, Hive, RegVal};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind")]
pub enum Change {
    Reg { hive: Hive, path: String, name: String, before: Option<RegVal>, after: Option<RegVal> },
    Wallpaper { before: String, after: String },
    TaskbarAutohide { before: bool, after: bool },
    DesktopIcons { before: bool, after: bool },
    PowerPlan { before: String, after: String },
    Task { path: String, name: String, enabled_before: bool },
    ServiceStart { name: String, before: u32, after: u32 },
    AppRemoved { name: String },
    AppxRemoved { name: String },
    Files { bytes: u64 },
    Multi { items: Vec<Change> },
}

impl Change {
    pub fn undoable(&self) -> bool {
        match self {
            Change::Files { .. } | Change::AppxRemoved { .. } => false,
            Change::Multi { items } => items.iter().any(|c| c.undoable()),
            _ => true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Entry {
    pub id: u64,
    pub ts: i64,
    pub module: String,
    pub title: String,
    pub detail: String,
    pub change: Change,
    pub undone: bool,
    pub undoable: bool,
}

#[derive(Serialize, Deserialize, Default)]
pub struct ChangeLog {
    pub entries: Vec<Entry>,
    pub next: u64,
}

fn path() -> PathBuf {
    data_dir().join("changes.json")
}

impl ChangeLog {
    pub fn load() -> ChangeLog {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::write(path(), s);
        }
    }
}

/// Record a change in the global log.
pub fn record(module: &str, title: &str, detail: &str, change: Change) -> u64 {
    let mut log = lock(&core().log);
    log.next += 1;
    let id = log.next;
    let undoable = change.undoable();
    log.entries.push(Entry {
        id,
        ts: now(),
        module: module.into(),
        title: title.into(),
        detail: detail.into(),
        change,
        undone: false,
        undoable,
    });
    // keep the log bounded
    if log.entries.len() > 3000 {
        let cut = log.entries.len() - 3000;
        log.entries.drain(0..cut);
    }
    log.save();
    id
}

/// Write a registry value and return the change (None if it was already set).
pub fn reg(hive: Hive, path: &str, name: &str, val: RegVal) -> Result<Option<Change>, String> {
    let before = win::reg_get(hive, path, name);
    if before.as_ref() == Some(&val) {
        return Ok(None);
    }
    win::reg_set(hive, path, name, &val)?;
    Ok(Some(Change::Reg { hive, path: path.into(), name: name.into(), before, after: Some(val) }))
}

/// Delete a registry value and return the change (None if it didn't exist).
pub fn reg_del(hive: Hive, path: &str, name: &str) -> Result<Option<Change>, String> {
    let before = win::reg_get(hive, path, name);
    if before.is_none() {
        return Ok(None);
    }
    win::reg_delete(hive, path, name)?;
    Ok(Some(Change::Reg { hive, path: path.into(), name: name.into(), before, after: None }))
}

/// Record several changes as one entry. Returns None if nothing changed.
pub fn record_many(module: &str, title: &str, detail: &str, items: Vec<Change>) -> Option<u64> {
    if items.is_empty() {
        return None;
    }
    let change = if items.len() == 1 { items.into_iter().next().unwrap() } else { Change::Multi { items } };
    Some(record(module, title, detail, change))
}

fn undo_change(c: &Change) -> Result<(), String> {
    match c {
        Change::Reg { hive, path, name, before, .. } => {
            match before {
                Some(v) => win::reg_set(*hive, path, name, v)?,
                None => win::reg_delete(*hive, path, name)?,
            }
            Ok(())
        }
        Change::Wallpaper { before, .. } => {
            if before.is_empty() {
                Ok(())
            } else {
                win::set_wallpaper(before)
            }
        }
        Change::TaskbarAutohide { before, .. } => {
            win::set_taskbar_autohide(*before);
            Ok(())
        }
        Change::DesktopIcons { before, .. } => {
            win::set_desktop_icons(*before);
            Ok(())
        }
        Change::PowerPlan { before, .. } => crate::speed::set_active_plan(before),
        Change::Task { path, name, enabled_before } => crate::speed::set_task_enabled(path, name, *enabled_before),
        Change::ServiceStart { name, before, .. } => crate::speed::set_service_start(name, *before),
        Change::AppRemoved { name } => crate::apps::reinstall(name),
        Change::AppxRemoved { .. } => Err("Store apps can be reinstalled from the Microsoft Store".into()),
        Change::Files { .. } => Err("Deleted junk files can't be brought back".into()),
        Change::Multi { items } => {
            let mut errs = vec![];
            for i in items.iter().rev() {
                if i.undoable() {
                    if let Err(e) = undo_change(i) {
                        errs.push(e);
                    }
                }
            }
            if errs.is_empty() {
                Ok(())
            } else {
                Err(errs.join("; "))
            }
        }
    }
}

pub fn undo(id: u64) -> Result<String, String> {
    let entry = {
        let log = lock(&core().log);
        log.entries.iter().find(|e| e.id == id).cloned()
    }
    .ok_or("Change not found")?;
    if entry.undone {
        return Err("Already undone".into());
    }
    if !entry.undoable {
        return Err("This change can't be undone".into());
    }
    let res = undo_change(&entry.change);
    win::broadcast("ImmersiveColorSet");
    win::broadcast("TraySettings");
    {
        let mut log = lock(&core().log);
        if let Some(e) = log.entries.iter_mut().find(|e| e.id == id) {
            e.undone = true;
        }
        log.save();
    }
    res.map(|_| format!("Undid: {}", entry.title))
}

/// Revert every change Nightmoth made, newest first.
pub fn undo_all() -> (u32, Vec<String>) {
    let ids: Vec<u64> = {
        let log = lock(&core().log);
        log.entries.iter().rev().filter(|e| !e.undone && e.undoable).map(|e| e.id).collect()
    };
    let mut n = 0;
    let mut errs = vec![];
    for id in ids {
        match undo(id) {
            Ok(_) => n += 1,
            Err(e) => errs.push(e),
        }
    }
    (n, errs)
}
