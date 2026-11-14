//! Shared helpers for integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use image::{ImageBuffer, Rgb};
use photovault::archive::Archive;
use photovault::archive::import::{DupDecision, DupLocation, ImportResult};
use photovault::progress::JobCtl;
use uuid::Uuid;

/// Writes a small, unique test image (PNG or JPEG chosen by extension).
pub fn make_image(path: &Path, seed: u32) {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(32 + seed % 7, 24 + seed % 5, |x, y| {
            Rgb([
                (x * 7 + seed * 13) as u8,
                (y * 11 + seed * 3) as u8,
                ((x ^ y) + seed * 29) as u8,
            ])
        });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    img.save(path).unwrap();
}

/// Creates `n` images in `dir` and returns their paths.
pub fn make_images(dir: &Path, prefix: &str, n: u32, seed: u32) -> Vec<PathBuf> {
    (0..n)
        .map(|i| {
            let ext = if i % 2 == 0 { "png" } else { "jpg" };
            let p = dir.join(format!("{prefix}_{i:03}.{ext}"));
            make_image(&p, seed * 1000 + i);
            p
        })
        .collect()
}

/// Imports files into a collection, answering duplicate prompts with `answer`.
pub fn import(
    archive: &mut Archive,
    id: Uuid,
    files: Vec<PathBuf>,
    answer: DupDecision,
) -> ImportResult {
    let req = archive.import_request(id, files).unwrap();
    let ctl = JobCtl::new();
    let mut ask = |_: &Path, _: &DupLocation| Some((answer, true));
    let result = photovault::archive::import::import_files(&req, &ctl, &mut ask).unwrap();
    archive.add_photos(id, result.imported.clone()).unwrap();
    result
}

/// Every regular file below `root` (relative, `/`-separated) → bytes.
pub fn snapshot_files(root: &Path) -> std::collections::BTreeMap<String, (Vec<u8>, i64)> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            let rel = photovault::fsutil::to_slash(e.path().strip_prefix(root).unwrap());
            let meta = e.metadata().unwrap();
            (
                rel,
                (
                    std::fs::read(e.path()).unwrap(),
                    photovault::fsutil::mtime_secs(&meta),
                ),
            )
        })
        .collect()
}
