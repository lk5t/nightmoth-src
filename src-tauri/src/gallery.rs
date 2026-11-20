//! Built-in wallpaper gallery: "Nightmoth Originals", generated on the PC so the
//! installer stays small and every image is original.

use image::{DynamicImage, RgbImage};
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::Serialize;
use std::f32::consts::PI;
use std::path::PathBuf;

use crate::win::data_dir;

pub const W: u32 = 2560;
pub const H: u32 = 1440;

pub const DESIGNS: &[(&str, &str)] = &[
    ("void", "Void"),
    ("ember", "Ember"),
    ("neon", "Neon Night"),
    ("aurora", "Aurora"),
    ("graphite", "Graphite"),
    ("deepsea", "Deep Sea"),
    ("crimson", "Crimson Waves"),
    ("starfield", "Starfield"),
    ("mountains", "Midnight Peaks"),
];

#[derive(Serialize, Clone)]
pub struct GalleryItem {
    pub id: String,
    pub name: String,
    pub path: String,
    pub thumb: String,
}

fn dir() -> PathBuf {
    let d = data_dir().join("gallery");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn path_of(id: &str) -> PathBuf {
    dir().join(format!("{id}.jpg"))
}

pub fn list() -> Result<Vec<GalleryItem>, String> {
    let mut out = vec![];
    for (id, name) in DESIGNS {
        let p = path_of(id);
        let tp = dir().join(format!("{id}_thumb.jpg"));
        if !p.exists() {
            let img = render(id);
            crate::look::save_jpeg(&DynamicImage::ImageRgb8(img), &p, 92)?;
        }
        if !tp.exists() {
            let img = image::open(&p).map_err(|e| e.to_string())?;
            crate::look::save_jpeg(&img.thumbnail(480, 270), &tp, 85)?;
        }
        let thumb = crate::look::file_data_url(&tp)?;
        out.push(GalleryItem { id: id.to_string(), name: name.to_string(), path: p.to_string_lossy().to_string(), thumb });
    }
    Ok(out)
}

/// Make sure a gallery wallpaper exists and return its path.
pub fn ensure(id: &str) -> Result<String, String> {
    let p = path_of(id);
    if !p.exists() {
        let img = render(id);
        crate::look::save_jpeg(&DynamicImage::ImageRgb8(img), &p, 92)?;
    }
    Ok(p.to_string_lossy().to_string())
}

// ------------------------------------------------------------------ noise

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393)
        ^ (y as u32).wrapping_mul(668_265_263)
        ^ seed.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0x00ff_ffff) as f32 / 16_777_215.0
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn noise(x: f32, y: f32, seed: u32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let xf = smooth(x - xi as f32);
    let yf = smooth(y - yi as f32);
    let a = hash(xi, yi, seed);
    let b = hash(xi + 1, yi, seed);
    let c = hash(xi, yi + 1, seed);
    let d = hash(xi + 1, yi + 1, seed);
    let top = a + (b - a) * xf;
    let bot = c + (d - c) * xf;
    top + (bot - top) * yf
}

fn fbm(x: f32, y: f32, seed: u32, oct: u32) -> f32 {
    let (mut s, mut amp, mut f, mut tot) = (0.0, 1.0, 1.0, 0.0);
    for i in 0..oct {
        s += noise(x * f, y * f, seed + i * 31) * amp;
        tot += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    s / tot
}

fn glow(u: f32, v: f32, cx: f32, cy: f32, r: f32) -> f32 {
    let dx = (u - cx) * (W as f32 / H as f32);
    let dy = v - cy;
    (-(dx * dx + dy * dy) / (r * r)).exp()
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn add(a: [f32; 3], b: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}

fn finish(img: &mut [f32], seed: u32, vignette: f32) {
    let w = W as usize;
    for (i, px) in img.chunks_mut(3).enumerate() {
        let x = (i % w) as f32 / W as f32;
        let y = (i / w) as f32 / H as f32;
        let dx = x - 0.5;
        let dy = y - 0.5;
        let vig = 1.0 - vignette * (dx * dx + dy * dy) * 2.2;
        let grain = (hash(i as i32, 7, seed) - 0.5) * 0.022;
        for c in px.iter_mut() {
            *c = (*c * vig + grain).clamp(0.0, 1.0);
        }
    }
}

fn to_image(buf: Vec<f32>) -> RgbImage {
    let bytes: Vec<u8> = buf.iter().map(|v| (v.clamp(0.0, 1.0).powf(1.0 / 1.1) * 255.0) as u8).collect();
    RgbImage::from_raw(W, H, bytes).expect("buffer size")
}

fn shade(f: impl Fn(f32, f32) -> [f32; 3]) -> Vec<f32> {
    let mut buf = vec![0f32; (W * H * 3) as usize];
    for y in 0..H {
        let v = y as f32 / H as f32;
        for x in 0..W {
            let u = x as f32 / W as f32;
            let c = f(u, v);
            let i = ((y * W + x) * 3) as usize;
            buf[i] = c[0];
            buf[i + 1] = c[1];
            buf[i + 2] = c[2];
        }
    }
    buf
}

/// Additive soft disc.
fn splat(buf: &mut [f32], cx: f32, cy: f32, r: f32, col: [f32; 3], k: f32) {
    let x0 = ((cx - r * 2.0).max(0.0)) as u32;
    let x1 = ((cx + r * 2.0).min(W as f32 - 1.0)) as u32;
    let y0 = ((cy - r * 2.0).max(0.0)) as u32;
    let y1 = ((cy + r * 2.0).min(H as f32 - 1.0)) as u32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let a = (-(dx * dx + dy * dy) / (r * r)).exp() * k;
            let i = ((y * W + x) * 3) as usize;
            buf[i] += col[0] * a;
            buf[i + 1] += col[1] * a;
            buf[i + 2] += col[2] * a;
        }
    }
}

// ------------------------------------------------------------------ designs

pub fn render(id: &str) -> RgbImage {
    let mut rng = StdRng::seed_from_u64(id.bytes().map(|b| b as u64).sum::<u64>() * 7919);
    let seed: u32 = rng.gen();
    let aspect = W as f32 / H as f32;
    let mut buf = match id {
        "ember" => {
            let mut b = shade(|u, v| {
                let base = [0.025, 0.012, 0.01];
                let g = glow(u, v, 0.5, 1.15, 0.75);
                add(base, [0.55, 0.12, 0.03], g * 0.55)
            });
            for _ in 0..900 {
                let x = rng.gen::<f32>() * W as f32;
                let y = (rng.gen::<f32>().powf(0.6)) * H as f32;
                let r = 1.2 + rng.gen::<f32>().powf(4.0) * 7.0;
                let heat = y / H as f32;
                let col = mix([1.0, 0.25, 0.05], [1.0, 0.75, 0.35], rng.gen::<f32>());
                splat(&mut b, x, y, r, col, 0.25 + heat * 0.8 * rng.gen::<f32>());
            }
            b
        }
        "neon" => shade(|u, v| {
            let horizon = 0.62;
            if v < horizon {
                let sky = mix([0.10, 0.02, 0.16], [0.01, 0.0, 0.03], 1.0 - v / horizon);
                // sun with stripes
                let dx = (u - 0.5) * aspect;
                let dy = v - 0.47;
                let d = (dx * dx + dy * dy).sqrt();
                let mut c = add(sky, [0.9, 0.1, 0.55], glow(u, v, 0.5, 0.5, 0.32) * 0.35);
                if d < 0.17 {
                    let stripe = ((v - 0.40) * 90.0).sin() > -0.3 || v < 0.47;
                    if stripe {
                        let t = ((v - 0.30) / 0.17).clamp(0.0, 1.0);
                        c = mix([1.0, 0.85, 0.25], [1.0, 0.12, 0.55], t);
                    }
                }
                c
            } else {
                // perspective grid floor
                let z = 1.0 / (v - horizon + 0.02);
                let gx = ((u - 0.5) * z * 3.0).fract().abs();
                let gz = (z * 0.6).fract();
                let line_x = (1.0 - (gx.min(1.0 - gx) * 40.0 / z.max(1.0)).min(1.0)).max(0.0);
                let line_z = (1.0 - (gz.min(1.0 - gz) * 25.0).min(1.0)).max(0.0);
                let fade = ((v - horizon) * 3.0).min(1.0);
                let floor = [0.02, 0.0, 0.04];
                let l = (line_x.max(line_z)) * fade;
                add(add(floor, [0.0, 0.85, 1.0], l * 0.8), [1.0, 0.1, 0.6], glow(u, v, 0.5, horizon, 0.18) * 0.4)
            }
        }),
        "aurora" => shade(|u, v| {
            let base = mix([0.0, 0.02, 0.05], [0.01, 0.03, 0.06], v);
            let mut c = base;
            for (i, col) in [[0.1, 0.9, 0.6], [0.2, 0.6, 1.0], [0.6, 0.2, 0.9]].iter().enumerate() {
                let fi = i as f32;
                let center = 0.30 + fi * 0.08 + 0.07 * (u * 5.0 + fi * 1.7).sin() + 0.05 * (fbm(u * 3.0, fi, seed, 3) - 0.5);
                let band = (-((v - center) / (0.05 + fi * 0.01)).powi(2)).exp();
                let streak = fbm(u * 60.0, v * 2.0, seed + i as u32, 2);
                let below = if v > center { (-(v - center) * 12.0).exp() } else { 1.0 };
                c = add(c, *col, band * streak * below * 0.55);
            }
            // stars
            let s = hash((u * 1800.0) as i32, (v * 1000.0) as i32, seed);
            if s > 0.9985 && v < 0.6 {
                c = add(c, [1.0, 1.0, 1.0], 0.6);
            }
            c
        }),
        "graphite" => shade(|u, v| {
            let f = fbm(u * 2.4 * aspect, v * 2.4, seed, 5) * 18.0;
            let fr = f.fract();
            let line = (1.0 - (fr.min(1.0 - fr) * 16.0).min(1.0)).max(0.0);
            let major = if (f as i32) % 5 == 0 { 1.0 } else { 0.45 };
            let base = 0.035 + 0.02 * glow(u, v, 0.3, 0.3, 0.6);
            let g = base + line * 0.16 * major;
            [g, g, g * 1.04]
        }),
        "deepsea" => {
            let mut b = shade(|u, v| {
                let c = mix([0.0, 0.05, 0.12], [0.0, 0.0, 0.02], v);
                add(c, [0.05, 0.3, 0.6], glow(u, v, 0.4, 0.0, 0.6) * 0.25)
            });
            for _ in 0..140 {
                let x = rng.gen::<f32>() * W as f32;
                let y = rng.gen::<f32>() * H as f32;
                let r = 6.0 + rng.gen::<f32>().powf(2.0) * 60.0;
                let col = mix([0.2, 0.6, 1.0], [0.4, 1.0, 0.95], rng.gen::<f32>());
                splat(&mut b, x, y, r, col, 0.05 + rng.gen::<f32>() * 0.12);
            }
            b
        }
        "crimson" => shade(|u, v| {
            let mut c = [0.012, 0.004, 0.006];
            for i in 0..7 {
                let fi = i as f32;
                let y = 0.38 + fi * 0.075
                    + 0.06 * (u * PI * (1.4 + fi * 0.35) + fi * 1.3).sin()
                    + 0.02 * (u * PI * 5.0 + fi).sin();
                if v > y {
                    let depth = 1.0 - fi / 7.0;
                    c = [0.06 + 0.32 * depth * depth, 0.008 * depth, 0.015 * depth];
                    let edge = (-(v - y) * 60.0).exp();
                    c = add(c, [1.0, 0.2, 0.25], edge * 0.35 * depth);
                }
            }
            add(c, [0.5, 0.02, 0.05], glow(u, v, 0.75, 0.25, 0.35) * 0.12)
        }),
        "starfield" => shade(|u, v| {
            let neb = fbm(u * 2.5 * aspect, v * 2.5, seed, 6);
            let n2 = fbm(u * 4.0 * aspect + 9.0, v * 4.0, seed + 5, 4);
            let mut c = [0.008, 0.006, 0.015];
            c = add(c, [0.45, 0.05, 0.25], ((neb - 0.5) * 3.0).max(0.0).powf(1.6) * 0.5);
            c = add(c, [0.15, 0.1, 0.55], ((n2 - 0.55) * 3.0).max(0.0).powf(1.5) * 0.4);
            let s = hash((u * W as f32) as i32, (v * H as f32) as i32, seed);
            if s > 0.9992 {
                c = add(c, [1.0, 0.95, 0.9], 0.5 + (s - 0.9992) * 600.0);
            } else if s > 0.997 {
                c = add(c, [0.8, 0.85, 1.0], 0.15);
            }
            c
        }),
        "mountains" => shade(|u, v| {
            let mut c = mix([0.03, 0.04, 0.10], [0.0, 0.0, 0.015], (1.0 - v).powf(0.8));
            c = add(c, [0.85, 0.88, 1.0], glow(u, v, 0.72, 0.22, 0.045) * 0.9);
            c = add(c, [0.3, 0.35, 0.6], glow(u, v, 0.72, 0.22, 0.25) * 0.15);
            let s = hash((u * 1600.0) as i32, (v * 900.0) as i32, seed);
            if s > 0.9988 && v < 0.5 {
                c = add(c, [1.0, 1.0, 1.0], 0.5);
            }
            for i in 0..4u32 {
                let fi = i as f32;
                let n = fbm(u * (1.5 + fi * 0.8), fi * 3.0, seed + i, 5);
                let ridged = 1.0 - (2.0 * n - 1.0).abs();
                let ridge = 0.50 + fi * 0.11 - (ridged - 0.5) * (0.42 - fi * 0.06);
                if v > ridge {
                    let k = 0.11 - fi * 0.028;
                    c = [k * 0.55, k * 0.65, k * 1.15];
                    let mist = (-(v - ridge) * 10.0).exp() * (0.09 - fi * 0.015);
                    c = add(c, [0.45, 0.5, 0.8], mist);
                }
            }
            c
        }),
        // "void" and anything unknown
        _ => shade(|u, v| {
            let base = [0.012, 0.012, 0.014];
            let c = add(base, [0.8, 0.04, 0.03], glow(u, v, 0.78, 0.74, 0.45) * 0.5);
            let c = add(c, [0.5, 0.02, 0.05], glow(u, v, 0.12, 0.18, 0.25) * 0.08);
            let swirl = fbm(u * 3.0 * aspect, v * 3.0, seed, 5);
            add(c, [0.35, 0.02, 0.02], ((swirl - 0.55) * 2.5).max(0.0) * glow(u, v, 0.7, 0.7, 0.6) * 0.3)
        }),
    };
    finish(&mut buf, seed, 0.55);
    to_image(buf)
}
