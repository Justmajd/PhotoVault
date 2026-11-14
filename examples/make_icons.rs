//! Generates the app icon set in `assets/` (run: `cargo run --release --example make_icons`).
//!
//! Produces `icon.png` (1024²), `icon-256.png`, `icon.ico` (PNG-compressed
//! 16–256 px) and `icon.iconset/` for macOS `iconutil -c icns`.

use std::io::Write;

use image::{ImageBuffer, Rgba, RgbaImage, imageops::FilterType};

fn rounded_rect_sdf(x: f32, y: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (x - cx).abs() - (hw - r);
    let qy = (y - cy).abs() - (hh - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

fn in_triangle(px: f32, py: f32, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let s = |p: (f32, f32), q: (f32, f32), r: (f32, f32)| {
        (p.0 - r.0) * (q.1 - r.1) - (q.0 - r.0) * (p.1 - r.1)
    };
    let d1 = s((px, py), a, b);
    let d2 = s((px, py), b, c);
    let d3 = s((px, py), c, a);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

fn sample(x: f32, y: f32) -> [f32; 4] {
    // Background squircle with a diagonal gradient.
    let bg = rounded_rect_sdf(x, y, 512.0, 512.0, 430.0, 430.0, 190.0);
    if bg > 0.0 {
        return [0.0; 4];
    }
    let t = ((x + y) / 2048.0).clamp(0.0, 1.0);
    let top = [0.36, 0.55, 0.94];
    let bottom = [0.56, 0.38, 0.92];
    let mut c = [
        top[0] + (bottom[0] - top[0]) * t,
        top[1] + (bottom[1] - top[1]) * t,
        top[2] + (bottom[2] - top[2]) * t,
        1.0,
    ];
    let white = |c: &mut [f32; 4], a: f32| {
        for ch in c.iter_mut().take(3) {
            *ch = *ch * (1.0 - a) + a;
        }
    };
    // Photo card.
    let card = rounded_rect_sdf(x, y, 512.0, 530.0, 280.0, 230.0, 48.0);
    let inner = rounded_rect_sdf(x, y, 512.0, 530.0, 236.0, 186.0, 24.0);
    if card <= 0.0 && inner > 0.0 {
        white(&mut c, 1.0);
    }
    if inner <= 0.0 {
        // Sun.
        let d = ((x - 630.0).powi(2) + (y - 440.0).powi(2)).sqrt();
        if d < 44.0 {
            white(&mut c, 1.0);
        }
        // Mountains.
        let base = 716.0;
        if in_triangle(x, y, (276.0, base), (440.0, 470.0), (640.0, base)) {
            white(&mut c, 1.0);
        } else if in_triangle(x, y, (500.0, base), (630.0, 560.0), (748.0, base)) {
            white(&mut c, 0.78);
        }
    }
    c
}

fn render(size: u32) -> RgbaImage {
    let ss = 4;
    let scale = 1024.0 / size as f32;
    ImageBuffer::from_fn(size, size, |px, py| {
        let mut acc = [0.0f32; 4];
        for sy in 0..ss {
            for sx in 0..ss {
                let x = (px as f32 + (sx as f32 + 0.5) / ss as f32) * scale;
                let y = (py as f32 + (sy as f32 + 0.5) / ss as f32) * scale;
                let s = sample(x, y);
                for i in 0..3 {
                    acc[i] += s[i] * s[3];
                }
                acc[3] += s[3];
            }
        }
        let n = (ss * ss) as f32;
        let a = acc[3] / n;
        let rgb = |v: f32| {
            if a > 0.0 {
                ((v / n / a) * 255.0).round().clamp(0.0, 255.0) as u8
            } else {
                0
            }
        };
        Rgba([
            rgb(acc[0]),
            rgb(acc[1]),
            rgb(acc[2]),
            (a * 255.0).round() as u8,
        ])
    })
}

fn png_bytes(img: &RgbaImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .expect("png");
    out.into_inner()
}

fn main() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let big = render(1024);
    big.save(assets.join("icon.png")).expect("icon.png");
    let resized = |s: u32| image::imageops::resize(&big, s, s, FilterType::Lanczos3);
    resized(256)
        .save(assets.join("icon-256.png"))
        .expect("icon-256.png");

    // ICO with PNG-compressed entries.
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let pngs: Vec<Vec<u8>> = sizes.iter().map(|&s| png_bytes(&resized(s))).collect();
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, png) in sizes.iter().zip(&pngs) {
        let dim = if *s >= 256 { 0 } else { *s as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for png in &pngs {
        ico.extend_from_slice(png);
    }
    std::fs::File::create(assets.join("icon.ico"))
        .and_then(|mut f| f.write_all(&ico))
        .expect("icon.ico");

    // macOS iconset (convert with `iconutil -c icns assets/icon.iconset`).
    let set = assets.join("icon.iconset");
    std::fs::create_dir_all(&set).expect("iconset");
    for s in [16u32, 32, 128, 256, 512] {
        resized(s)
            .save(set.join(format!("icon_{s}x{s}.png")))
            .expect("iconset png");
        resized(s * 2)
            .save(set.join(format!("icon_{s}x{s}@2x.png")))
            .expect("iconset png");
    }
    println!("icons written to {}", assets.display());
}
