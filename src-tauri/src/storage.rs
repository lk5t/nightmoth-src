//! Storage finder (app side, your own files): the biggest files, duplicates and old
//! downloads in your folders. Deleting sends them to the Recycle Bin.

use serde::Serialize;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::win::{self, Hive};

#[derive(Serialize, Clone, Debug)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: i64,
}

#[derive(Serialize)]
pub struct Report {
    pub large: Vec<FileInfo>,
    /// groups of identical files (the first one is the one to keep)
    pub duplicates: Vec<Vec<FileInfo>>,
    pub old_downloads: Vec<FileInfo>,
    pub scanned: u64,
    pub seconds: f64,
}

fn shell_folder(guid_or_name: &str, fallback: &str) -> PathBuf {
    win::reg_str(Hive::HKCU, r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders", guid_or_name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(win::env("USERPROFILE")).join(fallback))
}

fn roots() -> Vec<PathBuf> {
    vec![
        shell_folder("Desktop", "Desktop"),
        shell_folder("Personal", "Documents"),
        shell_folder("{374DE290-123F-4565-9164-39C4925E467B}", "Downloads"),
        shell_folder("My Video", "Videos"),
        shell_folder("My Pictures", "Pictures"),
        shell_folder("My Music", "Music"),
    ]
}

fn info(p: &Path, m: &std::fs::Metadata) -> FileInfo {
    FileInfo {
        path: p.to_string_lossy().into(),
        name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        size: m.len(),
        modified: m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0),
    }
}

/// Hash of the start, middle and end (1 MB each) — enough to tell same-size files apart.
fn sample_hash(p: &str, size: u64) -> Option<u64> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let chunk = 1 << 20;
    let mut buf = vec![0u8; chunk];
    for off in [0, size / 2, size.saturating_sub(chunk as u64)] {
        f.seek(SeekFrom::Start(off)).ok()?;
        let n = f.read(&mut buf).ok()?;
        buf[..n].hash(&mut h);
    }
    size.hash(&mut h);
    Some(h.finish())
}

pub fn scan() -> Report {
    let start = Instant::now();
    let budget = Duration::from_secs(25);
    let mut files: Vec<FileInfo> = vec![];
    let mut scanned = 0u64;
    let downloads = shell_folder("{374DE290-123F-4565-9164-39C4925E467B}", "Downloads");
    let mut stack: Vec<(PathBuf, u32)> = roots().into_iter().filter(|r| r.is_dir()).map(|r| (r, 0)).collect();
    let mut seen_dirs = std::collections::HashSet::new();
    while let Some((d, depth)) = stack.pop() {
        if start.elapsed() > budget || !seen_dirs.insert(d.clone()) {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            let p = e.path();
            if ft.is_dir() {
                let n = e.file_name().to_string_lossy().to_lowercase();
                if depth < 10 && !n.starts_with('.') && n != "node_modules" && n != "$recycle.bin" {
                    stack.push((p, depth + 1));
                }
            } else if let Ok(m) = e.metadata() {
                scanned += 1;
                if m.len() >= 5_000_000 || p.starts_with(&downloads) {
                    files.push(info(&p, &m));
                }
            }
        }
    }

    let mut large: Vec<FileInfo> = files.iter().filter(|f| f.size >= 500_000_000).cloned().collect();
    large.sort_by(|a, b| b.size.cmp(&a.size));
    large.truncate(60);

    // duplicates: same size first, then same sampled content
    let mut by_size: HashMap<u64, Vec<&FileInfo>> = HashMap::new();
    for f in files.iter().filter(|f| f.size >= 20_000_000) {
        by_size.entry(f.size).or_default().push(f);
    }
    let mut duplicates = vec![];
    for (size, group) in by_size {
        if group.len() < 2 || start.elapsed() > budget * 2 {
            continue;
        }
        let mut by_hash: HashMap<u64, Vec<FileInfo>> = HashMap::new();
        for f in group {
            if let Some(h) = sample_hash(&f.path, size) {
                by_hash.entry(h).or_default().push(f.clone());
            }
        }
        for (_, mut g) in by_hash {
            if g.len() > 1 {
                g.sort_by_key(|f| f.modified); // keep the oldest (the original)
                duplicates.push(g);
            }
        }
    }
    duplicates.sort_by(|a, b| (b[0].size * (b.len() as u64 - 1)).cmp(&(a[0].size * (a.len() as u64 - 1))));
    duplicates.truncate(40);

    let cutoff = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) - 90 * 86400;
    let mut old_downloads: Vec<FileInfo> = files.into_iter().filter(|f| Path::new(&f.path).starts_with(&downloads) && f.modified < cutoff).collect();
    old_downloads.sort_by(|a, b| b.size.cmp(&a.size));
    old_downloads.truncate(80);

    Report { large, duplicates, old_downloads, scanned, seconds: start.elapsed().as_secs_f64() }
}

/// Send files to the Recycle Bin. Only files inside your own folders are accepted.
pub fn recycle(paths: &[String]) -> Result<usize, String> {
    use windows_sys::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW};
    let profile = PathBuf::from(win::env("USERPROFILE"));
    let allowed: Vec<PathBuf> = roots();
    let ok: Vec<&String> = paths
        .iter()
        .filter(|p| {
            let pb = Path::new(p);
            pb.is_file() && (allowed.iter().any(|r| pb.starts_with(r)) || pb.starts_with(&profile))
        })
        .collect();
    if ok.is_empty() {
        return Ok(0);
    }
    // double-NUL-terminated list
    let mut list: Vec<u16> = vec![];
    for p in &ok {
        list.extend(p.encode_utf16());
        list.push(0);
    }
    list.push(0);
    let mut op: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    op.wFunc = FO_DELETE;
    op.pFrom = list.as_ptr();
    op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as u16;
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 {
        return Err(format!("Windows couldn't move some files (code {r})"));
    }
    Ok(ok.len())
}
