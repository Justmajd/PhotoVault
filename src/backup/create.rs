//! Streaming zip writer with optional verification pass.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use chrono::{Datelike, Local, TimeZone, Timelike};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::manifest::{
    ARCHIVE_PREFIX, BACKUP_FORMAT, BACKUP_FORMAT_VERSION, BackupKind, BackupStats, MANIFEST_NAME,
    Manifest, ManifestFile, README_NAME, current_os, readme_text,
};
use super::{BackupError, Result};
use crate::archive::import::is_media;
use crate::archive::model::{ArchiveMeta, COLLECTION_FILE, now};
use crate::fsutil::{self, IO_BUF};
use crate::progress::JobCtl;

/// One file selected for a backup.
#[derive(Clone, Debug)]
pub struct PlanFile {
    /// `/`-separated path relative to the archive root.
    pub rel: String,
    pub abs: PathBuf,
    pub size: u64,
    pub mtime: i64,
    /// Hash reused from the base manifest (unchanged files of an incremental backup).
    pub known_sha: Option<String>,
    /// Whether the file is stored in this zip (always for full backups).
    pub include: bool,
}

/// What a backup will contain.
#[derive(Clone, Debug, Default)]
pub struct BackupPlan {
    pub files: Vec<PlanFile>,
    pub collections: usize,
    pub photos: usize,
    pub total_bytes: u64,
    /// Bytes actually written (smaller for incremental backups).
    pub included_bytes: u64,
    pub included_files: usize,
    pub kind: BackupKind,
    pub base_created_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Backup options.
#[derive(Clone, Debug)]
pub struct BackupOptions {
    pub verify: bool,
}

impl Default for BackupOptions {
    fn default() -> Self {
        Self { verify: true }
    }
}

/// Walks the archive (skipping hidden folders such as `.trash` and temp files).
/// With `base`, only files whose size or mtime changed are included.
pub fn plan_backup(root: &Path, base: Option<&Manifest>) -> Result<BackupPlan> {
    let mut plan = BackupPlan {
        kind: if base.is_some() {
            BackupKind::Incremental
        } else {
            BackupKind::Full
        },
        ..Default::default()
    };
    plan.base_created_at = base.map(|b| b.created_at);
    let base_index: HashMap<&str, &ManifestFile> = base
        .map(|b| b.state().iter().map(|f| (f.path.as_str(), f)).collect())
        .unwrap_or_default();
    for entry in walkdir::WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0 || !fsutil::is_hidden_name(&e.file_name().to_string_lossy())
        })
    {
        let entry = entry.map_err(|e| BackupError::Other(e.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel_path = entry
            .path()
            .strip_prefix(root)
            .map_err(|e| BackupError::Other(e.to_string()))?;
        let rel = fsutil::to_slash(rel_path);
        let meta = entry
            .metadata()
            .map_err(|e| BackupError::Other(e.to_string()))?;
        let name = entry.file_name().to_string_lossy();
        if name == COLLECTION_FILE {
            plan.collections += 1;
        } else if is_media(entry.path()) {
            plan.photos += 1;
        }
        let size = meta.len();
        let mtime = fsutil::mtime_secs(&meta);
        let zip_path = format!("{ARCHIVE_PREFIX}{rel}");
        let (include, known_sha) = match base_index.get(zip_path.as_str()) {
            Some(prev) if prev.size == size && prev.mtime == mtime => {
                (false, Some(prev.sha256.clone()))
            }
            _ => (true, None),
        };
        plan.total_bytes += size;
        if include {
            plan.included_bytes += size;
            plan.included_files += 1;
        }
        plan.files.push(PlanFile {
            rel,
            abs: entry.path().to_path_buf(),
            size,
            mtime,
            known_sha,
            include,
        });
    }
    Ok(plan)
}

fn zip_datetime(mtime: i64) -> zip::DateTime {
    let local = Local
        .timestamp_opt(mtime, 0)
        .single()
        .unwrap_or_else(Local::now);
    let year = local.year().clamp(1980, 2107) as u16;
    zip::DateTime::from_date_and_time(
        year,
        local.month() as u8,
        local.day() as u8,
        local.hour() as u8,
        local.minute() as u8,
        local.second().min(58) as u8,
    )
    .unwrap_or_default()
}

fn options_for(rel: &str, size: u64, mtime: i64) -> SimpleFileOptions {
    let lower = rel.to_lowercase();
    let text = [".json", ".md", ".txt", ".csv", ".xmp"]
        .iter()
        .any(|e| lower.ends_with(e));
    SimpleFileOptions::default()
        .compression_method(if text {
            CompressionMethod::Deflated
        } else {
            CompressionMethod::Stored
        })
        .last_modified_time(zip_datetime(mtime))
        .large_file(size >= u32::MAX as u64)
}

/// Writes a backup zip for the archive at `root` to `dest`.
///
/// The zip is written to `<dest>.partial` and renamed on success; on error or
/// cancel the partial file is deleted. Returns the manifest that was written.
pub fn create_backup(
    root: &Path,
    meta: &ArchiveMeta,
    plan: &BackupPlan,
    dest: &Path,
    options: &BackupOptions,
    ctl: &JobCtl,
) -> Result<Manifest> {
    let partial = PathBuf::from(format!("{}.partial", dest.display()));
    let _ = root;
    let result = write_zip(meta, plan, &partial, ctl);
    let manifest = match result {
        Ok(m) => m,
        Err(e) => {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
    };
    if options.verify
        && let Err(e) = verify_backup(&partial, ctl)
    {
        let _ = fs::remove_file(&partial);
        return Err(e);
    }
    if ctl.is_cancelled() {
        let _ = fs::remove_file(&partial);
        return Err(BackupError::Cancelled);
    }
    fs::rename(&partial, dest).map_err(BackupError::io(dest))?;
    Ok(manifest)
}

fn write_zip(
    meta: &ArchiveMeta,
    plan: &BackupPlan,
    partial: &Path,
    ctl: &JobCtl,
) -> Result<Manifest> {
    let file = File::create(partial).map_err(BackupError::io(partial))?;
    let mut zip = ZipWriter::new(BufWriter::with_capacity(IO_BUF, file));

    let mut manifest = Manifest {
        format: BACKUP_FORMAT.to_owned(),
        format_version: BACKUP_FORMAT_VERSION,
        app_version: crate::APP_VERSION.to_owned(),
        created_at: now(),
        source_os: current_os().to_owned(),
        archive_id: meta.archive_id,
        archive_name: meta.name.clone(),
        stats: BackupStats {
            collections: plan.collections,
            photos: plan.photos,
            total_bytes: plan.total_bytes,
        },
        files: Vec::new(),
        kind: plan.kind,
        base_created_at: plan.base_created_at,
        snapshot: Vec::new(),
    };

    let text_opts = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip_datetime(manifest.created_at.timestamp()));
    zip.start_file(README_NAME, text_opts)?;
    zip.write_all(readme_text(&manifest).as_bytes())
        .map_err(BackupError::io(partial))?;

    ctl.begin_phase(
        "Writing backup",
        plan.included_files as u64,
        plan.included_bytes,
    );
    let mut buf = vec![0u8; IO_BUF];
    for f in &plan.files {
        let zip_path = format!("{ARCHIVE_PREFIX}{}", f.rel);
        if !f.include {
            manifest.snapshot.push(ManifestFile {
                path: zip_path,
                size: f.size,
                sha256: f.known_sha.clone().unwrap_or_default(),
                mtime: f.mtime,
            });
            continue;
        }
        if ctl.is_cancelled() {
            return Err(BackupError::Cancelled);
        }
        ctl.set_current(&f.rel);
        let mut input = File::open(&f.abs).map_err(BackupError::io(&f.abs))?;
        zip.start_file(zip_path.as_str(), options_for(&f.rel, f.size, f.mtime))?;
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        loop {
            if ctl.is_cancelled() {
                return Err(BackupError::Cancelled);
            }
            let n = input.read(&mut buf).map_err(BackupError::io(&f.abs))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            zip.write_all(&buf[..n]).map_err(BackupError::io(partial))?;
            size += n as u64;
            ctl.add_bytes(n as u64);
        }
        let entry = ManifestFile {
            path: zip_path,
            size,
            sha256: hex::encode(hasher.finalize()),
            mtime: f.mtime,
        };
        if plan.kind == BackupKind::Incremental {
            manifest.snapshot.push(entry.clone());
        }
        manifest.files.push(entry);
        ctl.inc_done();
    }

    zip.start_file(MANIFEST_NAME, text_opts)?;
    let json =
        serde_json::to_vec_pretty(&manifest).map_err(|e| BackupError::Manifest(e.to_string()))?;
    zip.write_all(&json).map_err(BackupError::io(partial))?;
    let writer = zip.finish()?;
    let file = writer
        .into_inner()
        .map_err(|e| BackupError::io(partial)(e.into_error()))?;
    file.sync_all().map_err(BackupError::io(partial))?;
    Ok(manifest)
}

/// Re-reads every archive entry, recomputes SHA-256 and compares with the manifest.
pub fn verify_backup(zip_path: &Path, ctl: &JobCtl) -> Result<()> {
    let manifest = super::restore::read_manifest(zip_path)?;
    let file = File::open(zip_path).map_err(BackupError::io(zip_path))?;
    let mut zip = ZipArchive::new(file)?;
    let expected: HashMap<&str, &ManifestFile> = manifest
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();
    let total: u64 = manifest.files.iter().map(|f| f.size).sum();
    ctl.begin_phase("Verifying backup", manifest.files.len() as u64, total);
    let mut seen = 0usize;
    let mut failures = Vec::new();
    let mut buf = vec![0u8; IO_BUF];
    for i in 0..zip.len() {
        if ctl.is_cancelled() {
            return Err(BackupError::Cancelled);
        }
        let mut entry = zip.by_index(i)?;
        let name = entry.name()?.into_owned();
        let Some(want) = expected.get(name.as_str()) else {
            continue;
        };
        ctl.set_current(&name);
        seen += 1;
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        let read_ok = loop {
            match entry.read(&mut buf) {
                Ok(0) => break true,
                Ok(n) => {
                    hasher.update(&buf[..n]);
                    size += n as u64;
                    ctl.add_bytes(n as u64);
                }
                Err(_) => break false,
            }
        };
        if !read_ok || size != want.size || hex::encode(hasher.finalize()) != want.sha256 {
            failures.push(name);
        }
        ctl.inc_done();
    }
    if seen != manifest.files.len() {
        let present: std::collections::HashSet<String> = zip
            .file_names()
            .filter_map(|n| n.ok().map(|n| n.into_owned()))
            .collect();
        failures.extend(
            manifest
                .files
                .iter()
                .filter(|f| !present.contains(&f.path))
                .map(|f| f.path.clone()),
        );
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(BackupError::VerificationFailed(failures))
    }
}
