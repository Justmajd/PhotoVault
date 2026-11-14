//! Import pipeline: format detection, EXIF extraction, copy + hash, duplicates.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use chrono::{NaiveDate, NaiveDateTime};
use thiserror::Error;
use uuid::Uuid;

use super::model::{Gps, Photo, now};
use super::naming;
use crate::fsutil;
use crate::progress::JobCtl;

/// Formats PhotoVault can preview and import.
pub const PREVIEW_EXTS: [&str; 9] = [
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "jfif",
];
/// Formats PhotoVault archives but cannot preview (generic tile + badge).
pub const ARCHIVE_ONLY_EXTS: [&str; 15] = [
    "heic", "heif", "avif", "dng", "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2", "mp4", "mov",
    "m4v", "3gp",
];

/// How a file is handled by PhotoVault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    /// Decodable image with a thumbnail.
    Previewable,
    /// Archived only (RAW, HEIC, video…).
    ArchiveOnly,
    /// Skipped on import.
    Unsupported,
}

/// Lower-case extension of a path.
pub fn extension_lower(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Classifies a file by extension.
pub fn media_kind(path: &Path) -> MediaKind {
    let ext = extension_lower(path);
    if PREVIEW_EXTS.contains(&ext.as_str()) {
        MediaKind::Previewable
    } else if ARCHIVE_ONLY_EXTS.contains(&ext.as_str()) {
        MediaKind::ArchiveOnly
    } else {
        MediaKind::Unsupported
    }
}

/// Returns `true` for any file PhotoVault would import.
pub fn is_media(path: &Path) -> bool {
    media_kind(path) != MediaKind::Unsupported
}

/// Metadata extracted from a file's EXIF block and header.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaInfo {
    pub taken_at: Option<NaiveDateTime>,
    pub camera: Option<String>,
    pub gps: Option<Gps>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

fn ascii_field(exif: &exif::Exif, tag: exif::Tag) -> Option<String> {
    let field = exif.get_field(tag, exif::In::PRIMARY)?;
    match &field.value {
        exif::Value::Ascii(parts) => {
            let s: String = parts
                .iter()
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            let s = s
                .trim_matches(|c: char| c == '\0' || c.is_whitespace())
                .to_owned();
            (!s.is_empty()).then_some(s)
        }
        _ => None,
    }
}

fn gps_coord(exif: &exif::Exif, tag: exif::Tag, ref_tag: exif::Tag) -> Option<f64> {
    let field = exif.get_field(tag, exif::In::PRIMARY)?;
    let exif::Value::Rational(v) = &field.value else {
        return None;
    };
    if v.len() < 3 {
        return None;
    }
    let deg = v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0;
    if !deg.is_finite() {
        return None;
    }
    let r = ascii_field(exif, ref_tag).unwrap_or_default();
    let sign = if r.starts_with('S') || r.starts_with('W') {
        -1.0
    } else {
        1.0
    };
    Some(deg * sign)
}

/// Parses an EXIF date like `2026:11:14 10:22:05`.
pub fn parse_exif_datetime(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim();
    NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y:%m:%dT%H:%M:%S"))
        .ok()
}

/// EXIF orientation value (1–8), or 1 if unknown.
pub fn exif_orientation(path: &Path) -> u32 {
    let Ok(file) = File::open(path) else { return 1 };
    let Ok(exif) = exif::Reader::new().read_from_container(&mut BufReader::new(file)) else {
        return 1;
    };
    exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .unwrap_or(1)
}

/// Reads EXIF metadata and image dimensions. Never fails: missing data is `None`.
pub fn read_media_info(path: &Path) -> MediaInfo {
    let mut info = MediaInfo::default();
    let mut orientation = 1;
    if let Ok(file) = File::open(path)
        && let Ok(exif) = exif::Reader::new().read_from_container(&mut BufReader::new(file))
    {
        info.taken_at = [
            exif::Tag::DateTimeOriginal,
            exif::Tag::DateTimeDigitized,
            exif::Tag::DateTime,
        ]
        .into_iter()
        .find_map(|t| ascii_field(&exif, t).and_then(|s| parse_exif_datetime(&s)));
        let make = ascii_field(&exif, exif::Tag::Make);
        let model = ascii_field(&exif, exif::Tag::Model);
        info.camera = match (make, model) {
            (Some(make), Some(model)) => {
                if model.to_lowercase().starts_with(&make.to_lowercase()) {
                    Some(model)
                } else {
                    Some(format!("{make} {model}"))
                }
            }
            (a, b) => a.or(b),
        };
        let lat = gps_coord(&exif, exif::Tag::GPSLatitude, exif::Tag::GPSLatitudeRef);
        let lon = gps_coord(&exif, exif::Tag::GPSLongitude, exif::Tag::GPSLongitudeRef);
        if let (Some(lat), Some(lon)) = (lat, lon)
            && (lat != 0.0 || lon != 0.0)
        {
            info.gps = Some(Gps { lat, lon });
        }
        orientation = exif
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))
            .unwrap_or(1);
    }
    if media_kind(path) == MediaKind::Previewable
        && let Ok((w, h)) = image::image_dimensions(path)
    {
        // Orientations 5–8 rotate by 90°, so the displayed size is swapped.
        if (5..=8).contains(&orientation) {
            info.width = Some(h);
            info.height = Some(w);
        } else {
            info.width = Some(w);
            info.height = Some(h);
        }
    }
    info
}

/// Builds a [`Photo`] for a file already inside a collection folder.
pub fn photo_from_file(path: &Path, sha256: &str, original_name: &str) -> Photo {
    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let info = read_media_info(path);
    let mut p = Photo::new(&file_name, sha256, size);
    p.original_name = original_name.to_owned();
    p.taken_at = info.taken_at;
    p.camera = info.camera;
    p.gps = info.gps;
    p.width = info.width;
    p.height = info.height;
    p.added_at = now();
    p
}

/// Files found when expanding a set of dropped / picked paths.
#[derive(Clone, Debug, Default)]
pub struct CollectedFiles {
    /// Importable files, sorted by path.
    pub files: Vec<PathBuf>,
    /// Files with unknown extensions.
    pub unsupported: Vec<PathBuf>,
    /// Name of the single source folder, if exactly one folder was given.
    pub folder_name: Option<String>,
}

/// Expands folders recursively and classifies files. Hidden files are ignored.
pub fn collect_import_files(paths: &[PathBuf]) -> CollectedFiles {
    let mut out = CollectedFiles::default();
    let dirs: Vec<&PathBuf> = paths.iter().filter(|p| p.is_dir()).collect();
    if dirs.len() == 1 {
        out.folder_name = dirs[0]
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
    }
    for path in paths {
        if path.is_dir() {
            for entry in walkdir::WalkDir::new(path)
                .follow_links(false)
                .sort_by_file_name()
                .into_iter()
                .filter_entry(|e| {
                    e.depth() == 0 || !fsutil::is_hidden_name(&e.file_name().to_string_lossy())
                })
                .filter_map(Result::ok)
            {
                if entry.file_type().is_file() {
                    classify(entry.path(), &mut out);
                }
            }
        } else if path.is_file() {
            classify(path, &mut out);
        }
    }
    out.files.sort();
    out.files.dedup();
    out
}

fn classify(path: &Path, out: &mut CollectedFiles) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if fsutil::is_hidden_name(&name) {
        return;
    }
    if is_media(path) {
        out.files.push(path.to_path_buf());
    } else {
        out.unsupported.push(path.to_path_buf());
    }
}

/// Date suggestion for a set of files.
#[derive(Clone, Debug, PartialEq)]
pub struct DateSuggestion {
    pub date: NaiveDate,
    /// `true` if it came from EXIF `DateTimeOriginal`, `false` if from mtimes.
    pub from_metadata: bool,
    /// Files grouped by EXIF day (for "split into one collection per day").
    pub by_day: BTreeMap<NaiveDate, Vec<PathBuf>>,
}

/// Suggests a collection date: earliest EXIF date, else earliest mtime.
pub fn suggest_date(files: &[PathBuf], ctl: Option<&JobCtl>) -> Option<DateSuggestion> {
    let mut earliest_exif: Option<NaiveDateTime> = None;
    let mut earliest_mtime: Option<NaiveDateTime> = None;
    let mut by_day: BTreeMap<NaiveDate, Vec<PathBuf>> = BTreeMap::new();
    for f in files {
        if let Some(ctl) = ctl {
            if ctl.is_cancelled() {
                return None;
            }
            ctl.set_current(&f.file_name().unwrap_or_default().to_string_lossy());
        }
        let info = read_media_info(f);
        let mtime = fs::metadata(f)
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<chrono::Local>::from(t).naive_local());
        if let Some(t) = info.taken_at {
            earliest_exif = Some(earliest_exif.map_or(t, |e| e.min(t)));
        }
        if let Some(t) = mtime {
            earliest_mtime = Some(earliest_mtime.map_or(t, |e| e.min(t)));
        }
        if let Some(day) = info.taken_at.or(mtime).map(|t| t.date()) {
            by_day.entry(day).or_default().push(f.clone());
        }
        if let Some(ctl) = ctl {
            ctl.inc_done();
        }
    }
    match (earliest_exif, earliest_mtime) {
        (Some(t), _) => Some(DateSuggestion {
            date: t.date(),
            from_metadata: true,
            by_day,
        }),
        (None, Some(t)) => Some(DateSuggestion {
            date: t.date(),
            from_metadata: false,
            by_day,
        }),
        _ => None,
    }
}

/// Where an existing copy of a file lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DupLocation {
    pub collection_id: Uuid,
    pub collection_title: String,
    pub file_name: String,
}

/// The user's answer when a file already exists in another collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DupDecision {
    Skip,
    ImportAnyway,
    ShowExisting,
}

/// Everything the import job needs, captured from the archive up front.
#[derive(Clone, Debug)]
pub struct ImportRequest {
    /// Absolute path of the target collection folder.
    pub collection_dir: PathBuf,
    /// Target collection id.
    pub collection_id: Uuid,
    /// Files to import.
    pub files: Vec<PathBuf>,
    /// File names already used in the collection (lowercase).
    pub taken_names: HashSet<String>,
    /// Hashes already in the target collection.
    pub hashes_here: HashSet<String>,
    /// Hashes elsewhere in the archive.
    pub hashes_elsewhere: HashMap<String, DupLocation>,
}

/// Result of an import job.
#[derive(Clone, Debug, Default)]
pub struct ImportResult {
    pub collection_id: Option<Uuid>,
    pub imported: Vec<Photo>,
    pub duplicates_skipped: Vec<PathBuf>,
    pub unsupported: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
    pub show_existing: Option<DupLocation>,
    pub cancelled: bool,
}

/// Errors that abort an import entirely.
#[derive(Debug, Error)]
pub enum ImportError {
    #[error("collection folder does not exist: {0}")]
    MissingFolder(PathBuf),
}

/// Callback asked when a file already exists in another collection. Returns
/// the decision and whether to apply it to all further conflicts, or `None`
/// to abort the import.
pub type AskDuplicate<'a> = dyn FnMut(&Path, &DupLocation) -> Option<(DupDecision, bool)> + 'a;

/// Copies files into the collection folder: stream-copy + SHA-256 in one pass,
/// fsync, temp name → final name, preserved mtime.
///
/// `ask` is called when a file already exists in another collection and must
/// return the decision plus whether to apply it to all further conflicts.
/// A cancelled import keeps the files that were fully copied (they are
/// returned in `imported`) and never leaves partial files behind.
pub fn import_files(
    req: &ImportRequest,
    ctl: &JobCtl,
    ask: &mut AskDuplicate<'_>,
) -> Result<ImportResult, ImportError> {
    if !req.collection_dir.is_dir() {
        return Err(ImportError::MissingFolder(req.collection_dir.clone()));
    }
    let mut result = ImportResult {
        collection_id: Some(req.collection_id),
        ..Default::default()
    };
    let mut taken = req.taken_names.clone();
    let mut hashes_here = req.hashes_here.clone();
    let mut apply_all: Option<DupDecision> = None;

    let total_bytes: u64 = req
        .files
        .iter()
        .filter_map(|f| fs::metadata(f).ok())
        .map(|m| m.len())
        .sum();
    ctl.begin_phase("Importing", req.files.len() as u64, total_bytes);

    for src in &req.files {
        if ctl.is_cancelled() {
            result.cancelled = true;
            break;
        }
        let original_name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        ctl.set_current(&original_name);
        if !is_media(src) {
            result.unsupported.push(src.clone());
            ctl.inc_done();
            continue;
        }
        let src_meta = match fs::metadata(src) {
            Ok(m) => m,
            Err(e) => {
                result.failed.push((src.clone(), e.to_string()));
                ctl.inc_done();
                continue;
            }
        };
        let tmp = fsutil::temp_path_for(&req.collection_dir.join(&original_name));
        let (sha, size) = match fsutil::copy_and_hash(src, &tmp, Some(ctl)) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                result.cancelled = true;
                break;
            }
            Err(e) => {
                result.failed.push((src.clone(), e.to_string()));
                ctl.inc_done();
                continue;
            }
        };

        // Duplicate checks happen after the single copy+hash pass; a duplicate's
        // temp copy is simply discarded.
        let mut keep = true;
        if hashes_here.contains(&sha) {
            keep = false;
            result.duplicates_skipped.push(src.clone());
        } else if let Some(existing) = req.hashes_elsewhere.get(&sha) {
            let decision = match apply_all {
                Some(d) => d,
                None => match ask(src, existing) {
                    Some((d, all)) => {
                        if all {
                            apply_all = Some(d);
                        }
                        d
                    }
                    None => {
                        // Dialog aborted → treat as cancel.
                        let _ = fs::remove_file(&tmp);
                        result.cancelled = true;
                        break;
                    }
                },
            };
            match decision {
                DupDecision::ImportAnyway => {}
                DupDecision::Skip => {
                    keep = false;
                    result.duplicates_skipped.push(src.clone());
                }
                DupDecision::ShowExisting => {
                    keep = false;
                    result.duplicates_skipped.push(src.clone());
                    if result.show_existing.is_none() {
                        result.show_existing = Some(existing.clone());
                    }
                }
            }
        }
        if !keep {
            let _ = fs::remove_file(&tmp);
            ctl.inc_done();
            continue;
        }

        let final_name = naming::unique_name(
            &req.collection_dir,
            &naming::sanitize_file_name(&original_name),
            &taken,
            true,
        );
        let dst = req.collection_dir.join(&final_name);
        if let Err(e) = fs::rename(&tmp, &dst) {
            let _ = fs::remove_file(&tmp);
            result.failed.push((src.clone(), e.to_string()));
            ctl.inc_done();
            continue;
        }
        let _ = fsutil::set_mtime_secs(&dst, fsutil::mtime_secs(&src_meta));
        taken.insert(final_name.to_lowercase());
        hashes_here.insert(sha.clone());
        let mut photo = photo_from_file(&dst, &sha, &original_name);
        photo.size_bytes = size;
        result.imported.push(photo);
        ctl.inc_done();
    }
    fsutil::sync_dir(&req.collection_dir);
    Ok(result)
}
