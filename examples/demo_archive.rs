//! Builds a demo archive with generated photos (for screenshots and manual testing).
//!
//! Usage: `cargo run --example demo_archive -- <path>`

use chrono::NaiveDate;
use image::{ImageBuffer, Rgb};
use photovault::archive::Archive;
use photovault::archive::import::{self, DupDecision, DupLocation};
use photovault::progress::JobCtl;

fn landscape(path: &std::path::Path, seed: u32) {
    let (w, h) = (640u32, 427u32);
    let hue = |s: u32, k: u32| ((s * 47 + k * 89) % 255) as f32 / 255.0;
    let sky_top = [0.25 + 0.5 * hue(seed, 1), 0.45 + 0.4 * hue(seed, 2), 0.85];
    let sky_bottom = [0.98, 0.75 + 0.2 * hue(seed, 3), 0.55 + 0.3 * hue(seed, 4)];
    let ground = [0.2 + 0.5 * hue(seed, 5), 0.25 + 0.3 * hue(seed, 6), 0.2];
    let img = ImageBuffer::from_fn(w, h, |x, y| {
        let fx = x as f32 / w as f32;
        let fy = y as f32 / h as f32;
        let ridge = 0.55
            + 0.12 * ((fx * 6.0 + seed as f32).sin())
            + 0.05 * ((fx * 17.0 + seed as f32 * 0.7).sin());
        let sun = ((fx - 0.7).powi(2) + (fy - 0.3).powi(2)).sqrt();
        let c = if fy > ridge {
            let d = (fy - ridge) * 1.5;
            [
                ground[0] * (1.0 - d),
                ground[1] * (1.0 - d),
                ground[2] * (1.0 - d),
            ]
        } else if sun < 0.06 {
            [1.0, 0.95, 0.8]
        } else {
            let t = fy / ridge;
            [
                sky_top[0] + (sky_bottom[0] - sky_top[0]) * t,
                sky_top[1] + (sky_bottom[1] - sky_top[1]) * t,
                sky_top[2] + (sky_bottom[2] - sky_top[2]) * t,
            ]
        };
        Rgb([
            (c[0] * 255.0) as u8,
            (c[1] * 255.0) as u8,
            (c[2] * 255.0) as u8,
        ])
    });
    img.save(path).expect("save");
}

fn main() {
    let root =
        std::path::PathBuf::from(std::env::args().nth(1).expect("usage: demo_archive <path>"));
    let src = root.with_extension("src");
    let mut archive = Archive::create(&root, "Family Archive").expect("create");
    let collections = [
        (
            "Wadi Rum Camping",
            (2025, 8, 2),
            9,
            "Two nights under the stars in the desert. The sand turned red at sunset and we climbed the dunes before breakfast.\n\nBedouin tea, a jeep tour and the quietest night of the year.",
            vec!["travel", "family"],
            true,
        ),
        (
            "رحلة البتراء",
            (2024, 4, 12),
            6,
            "زرنا الخزنة في الصباح الباكر قبل وصول الزوار. كان الجو جميلاً جداً.",
            vec!["سفر", "عائلة"],
            false,
        ),
        (
            "Graduation Day 🎓",
            (2026, 11, 14),
            7,
            "Finally! Mum cried, Dad took 400 photos.",
            vec!["university", "family"],
            true,
        ),
        (
            "Dead Sea Weekend",
            (2025, 11, 14),
            5,
            "Floating, mud, and a very salty sunset.",
            vec!["travel"],
            false,
        ),
        (
            "Grandma's 80th Birthday",
            (2023, 6, 21),
            8,
            "Everyone came. Even uncle Sami from Dubai.",
            vec!["family"],
            false,
        ),
        (
            "Aqaba Diving",
            (2025, 3, 8),
            4,
            "",
            vec!["travel", "sea"],
            false,
        ),
    ];
    for (seed, (title, (y, m, d), n, notes, tags, fav)) in (1u32..).zip(collections) {
        let id = archive
            .create_collection(title, NaiveDate::from_ymd_opt(y, m, d).unwrap())
            .unwrap();
        let dir = src.join(format!("c{seed}"));
        std::fs::create_dir_all(&dir).unwrap();
        let files: Vec<_> = (0..n)
            .map(|i| {
                let p = dir.join(format!("IMG_{:04}.jpg", 4000 + seed * 10 + i));
                landscape(&p, seed * 13 + i);
                p
            })
            .collect();
        let req = archive.import_request(id, files).unwrap();
        let mut ask = |_: &std::path::Path, _: &DupLocation| Some((DupDecision::Skip, true));
        let r = import::import_files(&req, &JobCtl::new(), &mut ask).unwrap();
        archive.add_photos(id, r.imported).unwrap();
        let e = archive.get_mut(id).unwrap();
        e.data.notes = notes.to_owned();
        e.data.tags = tags.into_iter().map(String::from).collect();
        e.data.favorite = fav;
        if let Some(p) = e.data.photos.first_mut() {
            p.caption = "First light".into();
            p.favorite = true;
        }
        archive.save_collection(id, false).unwrap();
    }
    let _ = std::fs::remove_dir_all(&src);
    println!("demo archive at {}", root.display());
}
