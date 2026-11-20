//! Theme other apps with the wallpaper's colours (app side, your own settings files):
//! Windows Terminal and VS Code, plus an accent-coloured mouse pointer.
//! Every file is backed up once before Nightmoth first touches it.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::win::{self, user_dir, Hive, RegVal};

fn backup_once(p: &Path, tag: &str) {
    let dir = user_dir().join("backups");
    let _ = std::fs::create_dir_all(&dir);
    let b = dir.join(format!("{tag}.json"));
    if !b.exists() {
        let _ = std::fs::copy(p, b);
    }
}

/// JSON with comments (Terminal and VS Code allow them) → plain JSON.
fn strip_jsonc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b: Vec<char> = s.chars().collect();
    let (mut i, mut in_str) = (0, false);
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 1;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        } else if c == '/' && b.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        } else {
            out.push(c);
        }
        i += 1;
    }
    drop_trailing_commas(&out)
}

/// `[1, 2,]` / `{"a": 1,}` → valid JSON (outside strings only).
fn drop_trailing_commas(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut in_str = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 1;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == ',' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_whitespace() {
                j += 1;
            }
            if !(j < b.len() && (b[j] == '}' || b[j] == ']')) {
                out.push(c);
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

fn read_jsonc(p: &Path) -> Option<Value> {
    let s = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&strip_jsonc(&s)).ok()
}

fn shade(hex: &str, f: f32) -> String {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xB3141C);
    let ch = |s: u32| -> u32 {
        let c = ((v >> s) & 0xFF) as f32;
        (if f >= 0.0 { c + (255.0 - c) * f } else { c * (1.0 + f) }).round().clamp(0.0, 255.0) as u32
    };
    format!("#{:02X}{:02X}{:02X}", ch(16), ch(8), ch(0))
}

// ------------------------------------------------------------------ Windows Terminal

fn terminal_files() -> Vec<PathBuf> {
    let local = PathBuf::from(win::env("LOCALAPPDATA"));
    [
        local.join(r"Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json"),
        local.join(r"Packages\Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe\LocalState\settings.json"),
        local.join(r"Microsoft\Windows Terminal\settings.json"),
    ]
    .into_iter()
    .filter(|p| p.exists())
    .collect()
}

fn terminal(accent: &str) -> usize {
    let scheme = json!({
        "name": "Nightmoth",
        "background": "#0B0B0D", "foreground": "#E6E6E8", "cursorColor": accent, "selectionBackground": shade(accent, -0.35),
        "black": "#0B0B0D", "red": "#E0474F", "green": "#4FD18B", "yellow": "#E6C15A", "blue": "#5B8DEF", "purple": accent, "cyan": "#4CC9D6", "white": "#CFCFD4",
        "brightBlack": "#55555C", "brightRed": "#FF6B72", "brightGreen": "#7BE3A9", "brightYellow": "#F2D27C", "brightBlue": "#82A9F5", "brightPurple": shade(accent, 0.3), "brightCyan": "#7ADCE5", "brightWhite": "#FFFFFF"
    });
    let mut n = 0;
    for f in terminal_files() {
        let Some(mut v) = read_jsonc(&f) else { continue };
        backup_once(&f, "terminal-settings");
        if !v["schemes"].is_array() {
            v["schemes"] = json!([]);
        }
        let schemes = v["schemes"].as_array_mut().unwrap();
        schemes.retain(|s| s["name"] != "Nightmoth");
        schemes.push(scheme.clone());
        if !v["profiles"].is_object() {
            v["profiles"] = json!({});
        }
        if !v["profiles"]["defaults"].is_object() {
            v["profiles"]["defaults"] = json!({});
        }
        // only take over the colours if you haven't picked a scheme of your own
        let cur = v["profiles"]["defaults"]["colorScheme"].as_str().unwrap_or("");
        if cur.is_empty() || cur == "Nightmoth" || cur == "Campbell" {
            v["profiles"]["defaults"]["colorScheme"] = json!("Nightmoth");
        }
        if std::fs::write(&f, serde_json::to_string_pretty(&v).unwrap_or_default()).is_ok() {
            n += 1;
        }
    }
    n
}

// ------------------------------------------------------------------ VS Code

fn vscode(accent: &str) -> usize {
    let appdata = PathBuf::from(win::env("APPDATA"));
    let mut n = 0;
    for app in ["Code", "Code - Insiders", "VSCodium", "Cursor"] {
        let f = appdata.join(app).join(r"User\settings.json");
        if !f.exists() {
            continue;
        }
        let Some(mut v) = read_jsonc(&f) else { continue };
        backup_once(&f, &format!("{}-settings", app.to_lowercase().replace(' ', "")));
        if !v.is_object() {
            continue;
        }
        if !v["workbench.colorCustomizations"].is_object() {
            v["workbench.colorCustomizations"] = json!({});
        }
        let cc = v["workbench.colorCustomizations"].as_object_mut().unwrap();
        for (k, val) in [
            ("activityBar.activeBorder", accent.to_string()),
            ("activityBarBadge.background", accent.to_string()),
            ("focusBorder", shade(accent, -0.1)),
            ("button.background", shade(accent, -0.15)),
            ("button.hoverBackground", accent.to_string()),
            ("progressBar.background", accent.to_string()),
            ("tab.activeBorderTop", accent.to_string()),
            ("panelTitle.activeBorder", accent.to_string()),
            ("editorCursor.foreground", accent.to_string()),
            ("textLink.foreground", shade(accent, 0.25)),
            ("statusBarItem.remoteBackground", shade(accent, -0.15)),
            ("badge.background", shade(accent, -0.15)),
        ] {
            cc.insert(k.into(), json!(val));
        }
        if std::fs::write(&f, serde_json::to_string_pretty(&v).unwrap_or_default()).is_ok() {
            n += 1;
        }
    }
    n
}

pub fn theme_apps(accent: &str) -> usize {
    terminal(accent) + vscode(accent)
}

// ------------------------------------------------------------------ accent mouse pointer

const CURSORS: &str = r"Control Panel\Cursors";

fn arrow_png(accent: &str, size: u32) -> Vec<u8> {
    // the classic arrow, in a 12 x 19 unit box
    let poly: [(f32, f32); 7] = [(0.0, 0.0), (0.0, 16.6), (4.0, 12.8), (6.9, 19.0), (9.3, 18.0), (6.5, 11.9), (11.8, 11.9)];
    let scale = (size as f32 - 4.0) / 19.5;
    let pts: Vec<(f32, f32)> = poly.iter().map(|(x, y)| (x * scale + 1.5, y * scale + 1.5)).collect();
    let rgb = u32::from_str_radix(accent.trim_start_matches('#'), 16).unwrap_or(0xB3141C);
    let (r, g, b) = (((rgb >> 16) & 0xFF) as u8, ((rgb >> 8) & 0xFF) as u8, (rgb & 0xFF) as u8);
    let inside = |x: f32, y: f32| -> bool {
        let mut c = false;
        let n = pts.len();
        for i in 0..n {
            let (xi, yi) = pts[i];
            let (xj, yj) = pts[(i + n - 1) % n];
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                c = !c;
            }
        }
        c
    };
    let edge_dist = |x: f32, y: f32| -> f32 {
        let n = pts.len();
        (0..n)
            .map(|i| {
                let (ax, ay) = pts[i];
                let (bx, by) = pts[(i + 1) % n];
                let (dx, dy) = (bx - ax, by - ay);
                let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                ((x - ax - t * dx).powi(2) + (y - ay - t * dy).powi(2)).sqrt()
            })
            .fold(f32::MAX, f32::min)
    };
    let stroke = (size as f32 / 32.0).max(1.0) * 1.3;
    let mut img = image::RgbaImage::new(size, size);
    for py in 0..size {
        for px in 0..size {
            // 4x4 supersampling
            let (mut fill, mut line) = (0u32, 0u32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let (x, y) = (px as f32 + (sx as f32 + 0.5) / 4.0, py as f32 + (sy as f32 + 0.5) / 4.0);
                    let d = edge_dist(x, y);
                    if inside(x, y) && d > stroke {
                        fill += 1;
                    } else if d <= stroke {
                        line += 1;
                    }
                }
            }
            let a = ((fill + line) * 255 / 16) as u8;
            if a == 0 {
                continue;
            }
            let f = fill as f32 / (fill + line).max(1) as f32;
            let mix = |c: u8, o: u8| (c as f32 * f + o as f32 * (1.0 - f)).round() as u8;
            img.put_pixel(px, py, image::Rgba([mix(r, 245), mix(g, 245), mix(b, 247), a]));
        }
    }
    let mut png = vec![];
    let _ = image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    png
}

/// A .cur file is an .ico with a hotspot; Windows accepts PNG data inside.
fn write_cur(path: &Path, png: &[u8], size: u32, hot: (u16, u16)) -> std::io::Result<()> {
    let mut f = vec![0u8, 0, 2, 0, 1, 0];
    f.push(if size >= 256 { 0 } else { size as u8 });
    f.push(if size >= 256 { 0 } else { size as u8 });
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(&hot.0.to_le_bytes());
    f.extend_from_slice(&hot.1.to_le_bytes());
    f.extend_from_slice(&(png.len() as u32).to_le_bytes());
    f.extend_from_slice(&22u32.to_le_bytes());
    f.extend_from_slice(png);
    std::fs::write(path, f)
}

fn reload_cursors() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETCURSORS};
    unsafe {
        SystemParametersInfoW(SPI_SETCURSORS, 0, std::ptr::null_mut(), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE);
    }
}

pub fn set_cursor(accent: &str) -> Result<(), String> {
    let dir = user_dir().join("cursors");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let size = 48;
    let file = dir.join(format!("arrow-{}.cur", accent.trim_start_matches('#').to_lowercase()));
    write_cur(&file, &arrow_png(accent, size), size, (2, 2)).map_err(|e| e.to_string())?;
    // remember the pointer you had, once
    let saved = dir.join("previous.txt");
    if !saved.exists() {
        let prev = win::reg_str(Hive::HKCU, CURSORS, "Arrow").unwrap_or_default();
        let _ = std::fs::write(&saved, prev);
    }
    win::reg_set(Hive::HKCU, CURSORS, "Arrow", &RegVal::Str(file.to_string_lossy().into()))?;
    reload_cursors();
    Ok(())
}

pub fn reset_cursor() {
    let saved = user_dir().join("cursors").join("previous.txt");
    let prev = std::fs::read_to_string(&saved).unwrap_or_default();
    let _ = win::reg_set(Hive::HKCU, CURSORS, "Arrow", &RegVal::Str(prev));
    let _ = std::fs::remove_file(saved);
    reload_cursors();
}
