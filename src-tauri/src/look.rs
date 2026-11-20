//! Wallpaper-driven theming: analyse the image, then theme Windows around it.

use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::DynamicImage;
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::changelog::{self, Change};
use crate::config::Look;
use crate::state::{cfg, core, with_cfg};
use crate::toast;
use crate::win::{self, data_dir, Hive, RegVal};

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const ACCENT: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";
const DWM: &str = r"Software\Microsoft\Windows\DWM";
const ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const SEARCH: &str = r"Software\Microsoft\Windows\CurrentVersion\Search";
const LOCK_CSP: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\PersonalizationCSP";

// ------------------------------------------------------------------ images

pub fn save_jpeg(img: &DynamicImage, path: &Path, q: u8) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut w = std::io::BufWriter::new(f);
    let rgb = DynamicImage::ImageRgb8(img.to_rgb8());
    rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut w, q)).map_err(|e| e.to_string())
}

pub fn file_data_url(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

pub fn thumb(path: &str, max: u32) -> Result<String, String> {
    let img = image::open(path).map_err(|e| format!("Can't open image: {e}"))?;
    let t = DynamicImage::ImageRgb8(img.thumbnail(max, max).to_rgb8());
    let mut buf: Vec<u8> = vec![];
    t.write_with_encoder(JpegEncoder::new_with_quality(&mut buf, 82)).map_err(|e| e.to_string())?;
    Ok(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(buf)))
}

const IMAGE_EXT: &[&str] = &["jpg", "jpeg", "png", "bmp", "webp", "gif", "tif", "tiff"];

pub fn is_image(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXT.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Windows only reliably accepts jpg/png/bmp: convert anything else.
fn prepare_wallpaper(path: &str) -> Result<String, String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err("That image doesn't exist any more".into());
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if ["jpg", "jpeg", "png", "bmp"].contains(&ext.as_str()) {
        return Ok(path.to_string());
    }
    let img = image::open(p).map_err(|e| format!("Can't open image: {e}"))?;
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("wallpaper");
    let out = data_dir().join("converted").join(format!("{stem}.jpg"));
    save_jpeg(&img, &out, 94)?;
    Ok(out.to_string_lossy().to_string())
}

// ------------------------------------------------------------------ colour

type Rgb = [f32; 3];

fn to_hex(c: Rgb) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        (c[0] * 255.0).round() as u8,
        (c[1] * 255.0).round() as u8,
        (c[2] * 255.0).round() as u8
    )
}

pub fn from_hex(s: &str) -> Option<Rgb> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok().map(|v| v as f32 / 255.0);
    Some([p(0)?, p(2)?, p(4)?])
}

fn rgb_to_hsl(c: Rgb) -> (f32, f32, f32) {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let l = (max + min) / 2.0;
    if (max - min).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == c[0] {
        ((c[1] - c[2]) / d + if c[1] < c[2] { 6.0 } else { 0.0 }) / 6.0
    } else if max == c[1] {
        ((c[2] - c[0]) / d + 2.0) / 6.0
    } else {
        ((c[0] - c[1]) / d + 4.0) / 6.0
    };
    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> Rgb {
    if s <= 0.0 {
        return [l, l, l];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        if t < 0.0 {
            t += 1.0
        }
        if t > 1.0 {
            t -= 1.0
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0)]
}

fn luminance(c: Rgb) -> f32 {
    let lin = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Make an accent readable on dark/light Windows surfaces.
fn fix_accent(c: Rgb, dark: bool) -> Rgb {
    let (h, mut s, mut l) = rgb_to_hsl(c);
    if s > 0.08 {
        s = s.max(0.45);
    }
    if dark {
        l = l.clamp(0.42, 0.66);
        let mut out = hsl_to_rgb(h, s, l);
        while contrast(out, [0.12, 0.12, 0.12]) < 3.0 && l < 0.85 {
            l += 0.03;
            out = hsl_to_rgb(h, s, l);
        }
        out
    } else {
        l = l.clamp(0.30, 0.52);
        let mut out = hsl_to_rgb(h, s, l);
        while contrast(out, [0.95, 0.95, 0.95]) < 3.0 && l > 0.12 {
            l -= 0.03;
            out = hsl_to_rgb(h, s, l);
        }
        out
    }
}

// ------------------------------------------------------------------ analysis

#[derive(Serialize, Clone, Debug)]
pub struct Analysis {
    pub palette: Vec<String>,
    pub weights: Vec<f32>,
    pub brightness: f32,
    pub saturation: f32,
    pub dark: bool,
    pub style: String,
    pub accent: String,
    pub accent_index: usize,
    pub empty_area: String,
}

fn kmeans(px: &[Rgb], k: usize) -> Vec<(Rgb, usize)> {
    let mut sorted: Vec<Rgb> = px.to_vec();
    sorted.sort_by(|a, b| luminance(*a).partial_cmp(&luminance(*b)).unwrap_or(std::cmp::Ordering::Equal));
    let mut centers: Vec<Rgb> = (0..k).map(|i| sorted[(i * 2 + 1) * sorted.len() / (k * 2)]).collect();
    let mut assign = vec![0usize; px.len()];
    for _ in 0..14 {
        for (i, p) in px.iter().enumerate() {
            let mut best = 0;
            let mut bd = f32::MAX;
            for (j, c) in centers.iter().enumerate() {
                let d = (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2);
                if d < bd {
                    bd = d;
                    best = j;
                }
            }
            assign[i] = best;
        }
        let mut sums = vec![[0f32; 3]; k];
        let mut counts = vec![0usize; k];
        for (i, p) in px.iter().enumerate() {
            let a = assign[i];
            sums[a][0] += p[0];
            sums[a][1] += p[1];
            sums[a][2] += p[2];
            counts[a] += 1;
        }
        for j in 0..k {
            if counts[j] > 0 {
                let n = counts[j] as f32;
                centers[j] = [sums[j][0] / n, sums[j][1] / n, sums[j][2] / n];
            }
        }
    }
    let mut counts = vec![0usize; k];
    for a in &assign {
        counts[*a] += 1;
    }
    let mut out: Vec<(Rgb, usize)> = centers.into_iter().zip(counts).filter(|(_, n)| *n > 0).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

pub fn analyze(path: &str) -> Result<Analysis, String> {
    let img = image::open(path).map_err(|e| format!("Can't open image: {e}"))?;
    let small = img.thumbnail(160, 160).to_rgb8();
    let (w, h) = small.dimensions();
    let px: Vec<Rgb> = small.pixels().map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]).collect();
    if px.is_empty() {
        return Err("Empty image".into());
    }

    let brightness = px.iter().map(|p| 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]).sum::<f32>() / px.len() as f32;
    let saturation = px.iter().map(|p| rgb_to_hsl(*p).1).sum::<f32>() / px.len() as f32;
    let dark = brightness < 0.58;

    let clusters = kmeans(&px, 6);
    let total = px.len() as f32;
    let palette: Vec<Rgb> = clusters.iter().map(|c| c.0).collect();
    let weights: Vec<f32> = clusters.iter().map(|c| c.1 as f32 / total).collect();

    // pick the most characterful colour: saturated, not too dark/light, decently present
    let mut best = 0usize;
    let mut best_score = f32::MIN;
    for (i, c) in palette.iter().enumerate() {
        let (_, s, l) = rgb_to_hsl(*c);
        let mut score = s * 1.6 + weights[i].sqrt() * 0.8 - (l - 0.5).abs() * 0.9;
        if l < 0.07 || l > 0.95 {
            score -= 1.5;
        }
        if s < 0.12 {
            score -= 0.6;
        }
        if score > best_score {
            best_score = score;
            best = i;
        }
    }

    let style = if saturation < 0.16 {
        if dark { "Calm minimal" } else { "Soft minimal" }
    } else if saturation > 0.45 && rgb_to_hsl(palette[best]).1 > 0.6 {
        "Neon"
    } else if dark {
        "Moody"
    } else {
        "Bright"
    };

    // where is the image calm? (edge density in a 3x3 grid)
    let gray: Vec<f32> = px.iter().map(|p| 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]).collect();
    let mut cells = [0f32; 9];
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let i = (y * w + x) as usize;
            let gx = gray[i + 1] - gray[i - 1];
            let gy = gray[i + w as usize] - gray[i - w as usize];
            let cell = ((y * 3 / h).min(2) * 3 + (x * 3 / w).min(2)) as usize;
            cells[cell] += (gx * gx + gy * gy).sqrt();
        }
    }
    let names = ["top left", "top", "top right", "left", "center", "right", "bottom left", "bottom", "bottom right"];
    let calm = cells.iter().enumerate().min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|x| x.0).unwrap_or(0);

    Ok(Analysis {
        palette: palette.iter().map(|c| to_hex(*c)).collect(),
        weights,
        brightness,
        saturation,
        dark,
        style: style.into(),
        accent: to_hex(fix_accent(palette[best], dark)),
        accent_index: best,
        empty_area: names[calm].into(),
    })
}

// ------------------------------------------------------------------ apply

fn abgr(c: Rgb) -> u32 {
    let r = (c[0] * 255.0).round() as u32;
    let g = (c[1] * 255.0).round() as u32;
    let b = (c[2] * 255.0).round() as u32;
    0xFF00_0000 | (b << 16) | (g << 8) | r
}

fn argb(a: u32, c: Rgb) -> u32 {
    let r = (c[0] * 255.0).round() as u32;
    let g = (c[1] * 255.0).round() as u32;
    let b = (c[2] * 255.0).round() as u32;
    (a << 24) | (r << 16) | (g << 8) | b
}

/// 8 shades from light to dark, as Windows stores them (R,G,B,0).
fn accent_palette(c: Rgb) -> (Vec<u8>, Vec<Rgb>) {
    let (h, s, l) = rgb_to_hsl(c);
    let offsets = [0.30, 0.20, 0.10, 0.0, -0.10, -0.20, -0.30, -0.36];
    let shades: Vec<Rgb> = offsets.iter().map(|o| hsl_to_rgb(h, s, (l + o).clamp(0.04, 0.96))).collect();
    let mut bytes = vec![];
    for sh in &shades {
        bytes.push((sh[0] * 255.0).round() as u8);
        bytes.push((sh[1] * 255.0).round() as u8);
        bytes.push((sh[2] * 255.0).round() as u8);
        bytes.push(0);
    }
    (bytes, shades)
}

fn accent_changes(accent: Rgb, dark: bool) -> Result<Vec<Change>, String> {
    let (bytes, shades) = accent_palette(accent);
    let mut ch = vec![];
    let mut push = |r: Result<Option<Change>, String>| -> Result<(), String> {
        if let Some(c) = r? {
            ch.push(c);
        }
        Ok(())
    };
    push(changelog::reg(Hive::HKCU, PERSONALIZE, "AppsUseLightTheme", RegVal::Dword(if dark { 0 } else { 1 })))?;
    push(changelog::reg(Hive::HKCU, PERSONALIZE, "SystemUsesLightTheme", RegVal::Dword(if dark { 0 } else { 1 })))?;
    push(changelog::reg(Hive::HKCU, ACCENT, "AccentPalette", RegVal::Bin(bytes)))?;
    push(changelog::reg(Hive::HKCU, ACCENT, "AccentColorMenu", RegVal::Dword(abgr(shades[3]))))?;
    push(changelog::reg(Hive::HKCU, ACCENT, "StartColorMenu", RegVal::Dword(abgr(shades[5]))))?;
    push(changelog::reg(Hive::HKCU, DWM, "AccentColor", RegVal::Dword(abgr(shades[3]))))?;
    push(changelog::reg(Hive::HKCU, DWM, "ColorizationColor", RegVal::Dword(argb(0xC4, shades[3]))))?;
    push(changelog::reg(Hive::HKCU, DWM, "ColorizationAfterglow", RegVal::Dword(argb(0xC4, shades[3]))))?;
    push(changelog::reg(Hive::HKCU, DWM, "ColorPrevalence", RegVal::Dword(1)))?;
    // accent on Start + taskbar only looks right in dark mode
    push(changelog::reg(Hive::HKCU, PERSONALIZE, "ColorPrevalence", RegVal::Dword(if dark { 1 } else { 0 })))?;
    push(changelog::reg(Hive::HKCU, PERSONALIZE, "EnableTransparency", RegVal::Dword(1)))?;
    Ok(ch)
}

fn lock_screen_changes(wallpaper: &str) -> Vec<Change> {
    let dir = PathBuf::from(win::env("ProgramData")).join("Nightmoth");
    let out = dir.join("lockscreen.jpg");
    let img = match image::open(wallpaper) {
        Ok(i) => i,
        Err(_) => return vec![],
    };
    if save_jpeg(&img, &out, 92).is_err() {
        return vec![];
    }
    let p = out.to_string_lossy().to_string();
    let mut ch = vec![];
    for (name, val) in [
        ("LockScreenImagePath", RegVal::Str(p.clone())),
        ("LockScreenImageUrl", RegVal::Str(p.clone())),
        ("LockScreenImageStatus", RegVal::Dword(1)),
    ] {
        if let Ok(Some(c)) = changelog::reg(Hive::HKLM, LOCK_CSP, name, val) {
            ch.push(c);
        }
    }
    ch
}

#[derive(Serialize, Clone)]
pub struct Applied {
    pub path: String,
    pub analysis: Analysis,
    pub accent: String,
}

/// Set a wallpaper and theme Windows around it.
pub fn apply(path: &str, announce: bool) -> Result<Applied, String> {
    let wall = prepare_wallpaper(path)?;
    let a = analyze(&wall)?;
    let c = cfg();

    let mut accent_rgb = from_hex(&a.accent).unwrap_or([0.88, 0.02, 0.0]);
    if let Some(i) = c.looks.get(&wall).and_then(|l| l.accent_index) {
        if let Some(hex) = a.palette.get(i) {
            accent_rgb = fix_accent(from_hex(hex).unwrap_or(accent_rgb), a.dark);
        }
    }
    if let Some(lock) = c.lock_accent.as_deref().and_then(from_hex) {
        accent_rgb = fix_accent(lock, a.dark);
    }

    let mut changes = vec![];
    let before = win::current_wallpaper();
    win::set_wallpaper(&wall)?;
    if before != wall {
        changes.push(Change::Wallpaper { before, after: wall.clone() });
    }
    if c.auto_theme {
        changes.extend(accent_changes(accent_rgb, a.dark)?);
        if c.lock_screen_sync {
            changes.extend(lock_screen_changes(&wall));
        }
    }
    win::broadcast("ImmersiveColorSet");
    win::broadcast("WindowsThemeElement");

    let accent_hex = to_hex(accent_rgb);
    let name = Path::new(&wall).file_name().and_then(|n| n.to_str()).unwrap_or("wallpaper").to_string();
    changelog::record_many("Look", "Theme from wallpaper", &format!("{name} · accent {accent_hex}"), changes);

    let analysis = a.clone();
    with_cfg(|cf| {
        let entry = cf.looks.entry(wall.clone()).or_insert_with(Look::default);
        entry.accent = accent_hex.clone();
        entry.dark = analysis.dark;
        entry.palette = analysis.palette.clone();
        entry.style = analysis.style.clone();
        cf.current_wallpaper = wall.clone();
    });

    // Windows sometimes reverts the accent a moment later: check and re-apply.
    if c.auto_theme {
        let dark = a.dark;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let (bytes, _) = accent_palette(accent_rgb);
            if win::reg_get(Hive::HKCU, ACCENT, "AccentPalette") != Some(RegVal::Bin(bytes)) {
                let _ = accent_changes(accent_rgb, dark);
                win::broadcast("ImmersiveColorSet");
            }
        });
    }

    if announce {
        toast::show("theme", "Theme applied", &format!("{} look · accent {}", a.style, accent_hex));
    }
    Ok(Applied { path: wall, analysis: a, accent: accent_hex })
}

/// Pick the next palette colour as accent for the current wallpaper.
pub fn shuffle() -> Result<Applied, String> {
    let c = cfg();
    let wall = if c.current_wallpaper.is_empty() { win::current_wallpaper() } else { c.current_wallpaper.clone() };
    if wall.is_empty() {
        return Err("Pick a wallpaper first".into());
    }
    let a = analyze(&wall)?;
    let cur = c.looks.get(&wall).and_then(|l| l.accent_index).unwrap_or(a.accent_index);
    // next colour that isn't almost black/white/grey
    let mut next = cur;
    for step in 1..=a.palette.len() {
        let i = (cur + step) % a.palette.len();
        if let Some(rgb) = from_hex(&a.palette[i]) {
            let (_, s, l) = rgb_to_hsl(rgb);
            if s > 0.12 && l > 0.08 && l < 0.94 {
                next = i;
                break;
            }
        }
    }
    with_cfg(|cf| {
        cf.looks.entry(wall.clone()).or_insert_with(Look::default).accent_index = Some(next);
    });
    apply(&wall, true)
}

// ------------------------------------------------------------------ taskbar & desktop

#[derive(Serialize, Clone)]
pub struct DesktopState {
    pub autohide: bool,
    pub center: bool,
    pub search: u32,
    pub taskview: bool,
    pub widgets: bool,
    pub icons: bool,
}

pub fn desktop_state() -> DesktopState {
    DesktopState {
        autohide: win::taskbar_autohide(),
        center: win::reg_dword(Hive::HKCU, ADVANCED, "TaskbarAl").unwrap_or(1) == 1,
        search: win::reg_dword(Hive::HKCU, SEARCH, "SearchboxTaskbarMode").unwrap_or(2),
        taskview: win::reg_dword(Hive::HKCU, ADVANCED, "ShowTaskViewButton").unwrap_or(1) == 1,
        widgets: win::reg_dword(Hive::HKCU, ADVANCED, "TaskbarDa").unwrap_or(1) == 1,
        icons: win::desktop_icons_visible(),
    }
}

pub fn desktop_set(key: &str, value: &serde_json::Value) -> Result<(), String> {
    let b = value.as_bool().unwrap_or(false);
    let mut ch: Vec<Change> = vec![];
    let title;
    match key {
        "autohide" => {
            let before = win::taskbar_autohide();
            win::set_taskbar_autohide(b);
            ch.push(Change::TaskbarAutohide { before, after: b });
            title = if b { "Taskbar auto-hide on" } else { "Taskbar auto-hide off" };
        }
        "center" => {
            ch.extend(changelog::reg(Hive::HKCU, ADVANCED, "TaskbarAl", RegVal::Dword(b as u32))?);
            title = if b { "Taskbar centred" } else { "Taskbar left-aligned" };
        }
        "search" => {
            let v = value.as_u64().unwrap_or(1).min(3) as u32;
            ch.extend(changelog::reg(Hive::HKCU, SEARCH, "SearchboxTaskbarMode", RegVal::Dword(v))?);
            title = "Taskbar search changed";
        }
        "taskview" => {
            ch.extend(changelog::reg(Hive::HKCU, ADVANCED, "ShowTaskViewButton", RegVal::Dword(b as u32))?);
            title = if b { "Task View button shown" } else { "Task View button hidden" };
        }
        "widgets" => {
            ch.extend(
                changelog::reg(Hive::HKCU, ADVANCED, "TaskbarDa", RegVal::Dword(b as u32))
                    .map_err(|_| "Windows blocks changing the Widgets button on this version".to_string())?,
            );
            title = if b { "Widgets button shown" } else { "Widgets button hidden" };
        }
        "icons" => {
            let before = win::desktop_icons_visible();
            ch.extend(changelog::reg(Hive::HKCU, ADVANCED, "HideIcons", RegVal::Dword(if b { 0 } else { 1 }))?);
            win::set_desktop_icons(b);
            ch.push(Change::DesktopIcons { before, after: b });
            title = if b { "Desktop icons shown" } else { "Desktop icons hidden" };
        }
        _ => return Err(format!("Unknown setting {key}")),
    }
    win::broadcast("TraySettings");
    changelog::record_many("Look", title, "", ch);
    Ok(())
}

// ------------------------------------------------------------------ presets

pub fn preset(id: &str) -> Result<String, String> {
    let set = |k: &str, v: serde_json::Value| {
        let _ = desktop_set(k, &v);
    };
    use serde_json::json;
    let name = match id {
        "void" => {
            apply(&crate::gallery::ensure("void")?, false)?;
            set("autohide", json!(true));
            set("center", json!(true));
            set("search", json!(0));
            set("taskview", json!(false));
            set("widgets", json!(false));
            set("icons", json!(false));
            "Void"
        }
        "neon" => {
            apply(&crate::gallery::ensure("neon")?, false)?;
            set("autohide", json!(true));
            set("center", json!(true));
            set("search", json!(1));
            set("taskview", json!(false));
            set("widgets", json!(false));
            set("icons", json!(false));
            "Neon Night"
        }
        "clean" => {
            set("autohide", json!(false));
            set("center", json!(true));
            set("search", json!(1));
            set("taskview", json!(false));
            set("widgets", json!(false));
            set("icons", json!(false));
            "Clean"
        }
        "default" => {
            set("autohide", json!(false));
            set("center", json!(true));
            set("search", json!(2));
            set("taskview", json!(true));
            set("widgets", json!(true));
            set("icons", json!(true));
            "Windows default"
        }
        _ => return Err("Unknown preset".into()),
    };
    toast::show("theme", &format!("{name} preset applied"), "Your desktop just got a glow-up");
    Ok(name.into())
}

// ------------------------------------------------------------------ slideshow

pub fn folder_images(folder: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(folder)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file() && is_image(p))
                .map(|p| p.to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

pub fn slideshow_tick() {
    let c = cfg();
    if !c.slideshow_on || c.slideshow_folder.is_empty() {
        return;
    }
    if core().boosting.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let due = c.slideshow_last + (c.slideshow_minutes.max(1) as i64) * 60;
    if win::now() < due {
        return;
    }
    slideshow_next();
}

pub fn slideshow_next() {
    let c = cfg();
    let files = folder_images(&c.slideshow_folder);
    if files.is_empty() {
        return;
    }
    let idx = (c.slideshow_index + 1) % files.len();
    let _ = apply(&files[idx], true);
    with_cfg(|cf| {
        cf.slideshow_index = idx;
        cf.slideshow_last = win::now();
    });
}
