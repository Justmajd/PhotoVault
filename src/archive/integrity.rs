//! Integrity check: rehash every photo and compare with `collection.json`.

use std::collections::HashSet;
use std::path::PathBuf;

use uuid::Uuid;

use super::{Archive, CollectionEntry, Result, import, media_files_in};
use crate::fsutil;
use crate::progress::JobCtl;

/// A snapshot of one collection taken for the integrity job.
#[derive(Clone, Debug)]
pub struct IntegrityInput {
    pub collection_id: Uuid,
    pub title: String,
    pub dir: PathBuf,
    /// `(file name, expected sha256)`.
    pub photos: Vec<(String, String)>,
}

/// One problematic file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrityItem {
    pub collection_id: Uuid,
    pub collection_title: String,
    pub file_name: String,
    pub expected: String,
    /// Hash found on disk (`None` for missing files).
    pub actual: Option<String>,
}

/// Result of an integrity check.
#[derive(Clone, Debug, Default)]
pub struct IntegrityReport {
    pub ok: usize,
    pub modified: Vec<IntegrityItem>,
    pub missing: Vec<IntegrityItem>,
    pub orphans: Vec<IntegrityItem>,
    pub cancelled: bool,
}

impl IntegrityReport {
    /// `true` if nothing is wrong.
    pub fn is_clean(&self) -> bool {
        self.modified.is_empty() && self.missing.is_empty() && self.orphans.is_empty()
    }
}

/// Snapshot the archive for a background integrity check.
pub fn snapshot(archive: &Archive) -> Vec<IntegrityInput> {
    archive
        .collections
        .iter()
        .map(|c: &CollectionEntry| IntegrityInput {
            collection_id: c.data.id,
            title: c.data.title.clone(),
            dir: archive.root.join(&c.rel_dir),
            photos: c
                .data
                .photos
                .iter()
                .map(|p| (p.file_name.clone(), p.sha256.clone()))
                .collect(),
        })
        .collect()
}

/// Rehashes every photo. Runs on a background thread.
pub fn check(inputs: &[IntegrityInput], ctl: &JobCtl) -> IntegrityReport {
    let total_files: usize = inputs.iter().map(|i| i.photos.len()).sum();
    let total_bytes: u64 = inputs
        .iter()
        .flat_map(|i| i.photos.iter().map(move |(f, _)| i.dir.join(f)))
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    ctl.begin_phase("Verifying", total_files as u64, total_bytes);
    let mut report = IntegrityReport::default();
    for input in inputs {
        let listed: HashSet<&str> = input.photos.iter().map(|(f, _)| f.as_str()).collect();
        for (file, expected) in &input.photos {
            if ctl.is_cancelled() {
                report.cancelled = true;
                return report;
            }
            ctl.set_current(file);
            let path = input.dir.join(file);
            let item = |actual: Option<String>| IntegrityItem {
                collection_id: input.collection_id,
                collection_title: input.title.clone(),
                file_name: file.clone(),
                expected: expected.clone(),
                actual,
            };
            if !path.is_file() {
                report.missing.push(item(None));
            } else {
                match fsutil::sha256_file(&path, Some(ctl)) {
                    Ok(actual) if &actual == expected => report.ok += 1,
                    Ok(actual) => report.modified.push(item(Some(actual))),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                        report.cancelled = true;
                        return report;
                    }
                    Err(_) => report.missing.push(item(None)),
                }
            }
            ctl.inc_done();
        }
        for file in media_files_in(&input.dir) {
            if listed.contains(file.as_str()) {
                continue;
            }
            let actual = fsutil::sha256_file(&input.dir.join(&file), None).ok();
            report.orphans.push(IntegrityItem {
                collection_id: input.collection_id,
                collection_title: input.title.clone(),
                file_name: file,
                expected: String::new(),
                actual,
            });
        }
    }
    report
}

impl Archive {
    /// Adds orphan files to their collections' metadata.
    pub fn fix_adopt_orphans(&mut self, items: &[IntegrityItem]) -> Result<usize> {
        let mut n = 0;
        let mut touched = HashSet::new();
        for item in items {
            let Some(dir) = self.collection_dir(item.collection_id) else {
                continue;
            };
            let path = dir.join(&item.file_name);
            let Some(sha) = item
                .actual
                .clone()
                .or_else(|| fsutil::sha256_file(&path, None).ok())
            else {
                continue;
            };
            let photo = import::photo_from_file(&path, &sha, &item.file_name);
            if let Some(entry) = self.get_mut(item.collection_id)
                && entry.data.photo_index(&item.file_name).is_none()
            {
                entry.data.photos.push(photo);
                touched.insert(item.collection_id);
                n += 1;
            }
        }
        for id in touched {
            self.save_collection(id, false)?;
        }
        Ok(n)
    }

    /// Drops metadata entries whose files are missing.
    pub fn fix_drop_missing(&mut self, items: &[IntegrityItem]) -> Result<usize> {
        let mut n = 0;
        let mut touched = HashSet::new();
        for item in items {
            if let Some(entry) = self.get_mut(item.collection_id)
                && let Some(i) = entry.data.photo_index(&item.file_name)
            {
                entry.data.photos.remove(i);
                entry.missing.remove(&item.file_name);
                if entry.data.cover_photo.as_deref() == Some(item.file_name.as_str()) {
                    entry.data.cover_photo = None;
                }
                touched.insert(item.collection_id);
                n += 1;
            }
        }
        for id in touched {
            self.save_collection(id, false)?;
        }
        Ok(n)
    }

    /// Accepts the on-disk hash of modified files.
    pub fn fix_update_hashes(&mut self, items: &[IntegrityItem]) -> Result<usize> {
        let mut n = 0;
        let mut touched = HashSet::new();
        for item in items {
            let Some(actual) = &item.actual else { continue };
            let Some(path) = self.photo_path(item.collection_id, &item.file_name) else {
                continue;
            };
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if let Some(entry) = self.get_mut(item.collection_id)
                && let Some(i) = entry.data.photo_index(&item.file_name)
            {
                entry.data.photos[i].sha256 = actual.clone();
                entry.data.photos[i].size_bytes = size;
                touched.insert(item.collection_id);
                n += 1;
            }
        }
        for id in touched {
            self.save_collection(id, false)?;
        }
        Ok(n)
    }
}
