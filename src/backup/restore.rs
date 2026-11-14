//! Restore: preview, safe extraction (zip-slip protection), verification and
//! an atomic move into place.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use zip::ZipArchive;

use super::manifest::{
    ARCHIVE_PREFIX, BACKUP_FORMAT, BACKUP_FORMAT_VERSION, BackupKind, MANIFEST_NAME, Manifest,
    ManifestFile,
};
use super::{BackupError, Result};
use crate::archive::model::{ARCHIVE_FILE, ArchiveMeta, TRASH_DIR};
use crate::fsutil::{self, IO_BUF};
use crate::progress::JobCtl;

/// Name of the staging folder used while restoring.
pub const STAGING_DIR: &str = ".photovault-restoring";
/// Default sub-folder when the chosen destination is not empty.
pub const RESTORED_SUBFOLDER: &str = "PhotoVault Archive (Restored)";

/// Outcome of a successful restore.
#[derive(Clone, Debug, Default)]
pub struct RestoreReport {
    pub root: PathBuf,
    pub files: usize,
    pub bytes: u64,
    pub collections: usize,
    pub photos: usize,
}

/// Reads and validates `manifest.json` from a backup zip.
pub fn read_manifest(zip_path: &Path) -> Result<Manifest> {
    let file = File::open(zip_path).map_err(BackupError::io(zip_path))?;
    let mut zip = ZipArchive::new(file).map_err(|_| BackupError::NotABackup)?;
    let mut entry = zip
        .by_name(MANIFEST_NAME)
        .map_err(|_| BackupError::NotABackup)?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(BackupError::io(zip_path))?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| BackupError::Manifest(e.to_string()))?;
    if value.get("format").and_then(|v| v.as_str()) != Some(BACKUP_FORMAT) {
        return Err(BackupError::NotABackup);
    }
    let version = value
        .get("format_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if version > BACKUP_FORMAT_VERSION {
        return Err(BackupError::UnsupportedVersion(version));
    }
    serde_json::from_value(value).map_err(|e| BackupError::Manifest(e.to_string()))
}

/// Validates an entry name: relative, `/`-separated, no `..`, no drive letters.
pub fn check_entry_name(name: &str) -> Result<PathBuf> {
    let unsafe_path = || BackupError::UnsafePath(name.to_owned());
    if name.is_empty() || name.starts_with('/') || name.contains('\\') || name.contains('\0') {
        return Err(unsafe_path());
    }
    if name.len() >= 2 && name.as_bytes()[1] == b':' {
        return Err(unsafe_path());
    }
    let mut out = PathBuf::new();
    for part in name.split('/') {
        match part {
            "" | "." => continue,
            ".." => return Err(unsafe_path()),
            p => out.push(p),
        }
    }
    if out.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(unsafe_path());
    }
    Ok(out)
}

/// Returns `dest` if it is missing or empty, otherwise a fresh
/// `PhotoVault Archive (Restored)` sub-folder inside it.
pub fn resolve_destination(dest: &Path) -> PathBuf {
    let empty = match fs::read_dir(dest) {
        Ok(mut rd) => rd.next().is_none(),
        Err(_) => true,
    };
    if empty {
        return dest.to_path_buf();
    }
    let name =
        crate::archive::naming::unique_name(dest, RESTORED_SUBFOLDER, &HashSet::new(), false);
    dest.join(name)
}

fn dir_is_empty(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|mut rd| rd.next().is_none())
        .unwrap_or(true)
}

/// Extracts all `archive/` entries into `staging`, verifying each against the
/// manifest. Collects every failure instead of stopping at the first one.
fn extract_verified(
    zip_path: &Path,
    manifest: &Manifest,
    staging: &Path,
    ctl: &JobCtl,
) -> Result<(usize, u64)> {
    let file = File::open(zip_path).map_err(BackupError::io(zip_path))?;
    let mut zip = ZipArchive::new(file)?;
    let expected: HashMap<&str, &ManifestFile> = manifest
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();
    let total: u64 = manifest.files.iter().map(|f| f.size).sum();
    ctl.begin_phase("Restoring", manifest.files.len() as u64, total);

    // First pass: validate every name before writing anything.
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i)?;
        let name = entry.name()?.into_owned();
        check_entry_name(&name)?;
        if entry.enclosed_name().is_none() {
            return Err(BackupError::UnsafePath(name));
        }
    }

    let mut failures = Vec::new();
    let mut seen = HashSet::new();
    let mut bytes = 0u64;
    let mut buf = vec![0u8; IO_BUF];
    for i in 0..zip.len() {
        if ctl.is_cancelled() {
            return Err(BackupError::Cancelled);
        }
        let mut entry = zip.by_index(i)?;
        let name = entry.name()?.into_owned();
        if entry.is_dir() || !name.starts_with(ARCHIVE_PREFIX) {
            continue;
        }
        let Some(want) = expected.get(name.as_str()) else {
            return Err(BackupError::Manifest(format!(
                "{name} is in the zip but not in manifest.json"
            )));
        };
        ctl.set_current(&name);
        let rel = check_entry_name(&name[ARCHIVE_PREFIX.len()..])?;
        let out_path = staging.join(&rel);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(BackupError::io(parent))?;
        }
        let mut out = File::create_new(&out_path).map_err(BackupError::io(&out_path))?;
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        let read_ok = loop {
            match entry.read(&mut buf) {
                Ok(0) => break true,
                Ok(n) => {
                    hasher.update(&buf[..n]);
                    out.write_all(&buf[..n])
                        .map_err(BackupError::io(&out_path))?;
                    size += n as u64;
                    ctl.add_bytes(n as u64);
                }
                Err(e) => {
                    log::warn!("read error in {name}: {e}");
                    break false;
                }
            }
        };
        out.sync_all().map_err(BackupError::io(&out_path))?;
        drop(out);
        if !read_ok || size != want.size || hex::encode(hasher.finalize()) != want.sha256 {
            failures.push(name.clone());
            let _ = fs::remove_file(&out_path);
        } else {
            let _ = fsutil::set_mtime_secs(&out_path, want.mtime);
            bytes += size;
        }
        seen.insert(name);
        ctl.inc_done();
    }
    for f in &manifest.files {
        if !seen.contains(&f.path) {
            failures.push(format!("{} (missing from zip)", f.path));
        }
    }
    if !failures.is_empty() {
        return Err(BackupError::VerificationFailed(failures));
    }
    Ok((seen.len(), bytes))
}

/// Restores a full backup into `dest`, which must be empty or not exist.
pub fn restore_backup(zip_path: &Path, dest: &Path, ctl: &JobCtl) -> Result<RestoreReport> {
    let manifest = read_manifest(zip_path)?;
    if manifest.kind == BackupKind::Incremental {
        return Err(BackupError::Other(
            "this is an incremental backup; restore the full backup first, then apply this one onto it".into(),
        ));
    }
    let created_dest = !dest.exists();
    if !created_dest && !dir_is_empty(dest) {
        return Err(BackupError::DestinationNotEmpty(dest.to_path_buf()));
    }
    fs::create_dir_all(dest).map_err(BackupError::io(dest))?;
    let staging = dest.join(STAGING_DIR);
    let cleanup = |staging: &Path| {
        let _ = fs::remove_dir_all(staging);
        if created_dest && dir_is_empty(dest) {
            let _ = fs::remove_dir(dest);
        }
    };
    if let Err(e) = fs::create_dir_all(&staging) {
        cleanup(&staging);
        return Err(BackupError::io(&staging)(e));
    }
    let (files, bytes) = match extract_verified(zip_path, &manifest, &staging, ctl) {
        Ok(v) => v,
        Err(e) => {
            cleanup(&staging);
            return Err(e);
        }
    };
    // Everything verified: move the top-level items into place.
    let mut moved = Vec::new();
    for entry in fs::read_dir(&staging).map_err(BackupError::io(&staging))? {
        let entry = entry.map_err(BackupError::io(&staging))?;
        let target = dest.join(entry.file_name());
        if let Err(e) = fs::rename(entry.path(), &target) {
            // Roll back what we moved so the destination stays clean.
            for (from, to) in moved.iter().rev() {
                let _ = fs::rename(to, from);
            }
            cleanup(&staging);
            return Err(BackupError::io(&target)(e));
        }
        moved.push((entry.path(), target));
    }
    let _ = fs::remove_dir_all(&staging);
    fsutil::sync_dir(dest);
    Ok(RestoreReport {
        root: dest.to_path_buf(),
        files,
        bytes,
        collections: manifest.stats.collections,
        photos: manifest.stats.photos,
    })
}

/// Applies an incremental backup onto an existing archive restored from the
/// earlier backups of the same chain. Changed files are replaced, deleted
/// files are moved to the archive's trash.
pub fn apply_incremental(
    zip_path: &Path,
    archive_root: &Path,
    ctl: &JobCtl,
) -> Result<RestoreReport> {
    let manifest = read_manifest(zip_path)?;
    if manifest.kind != BackupKind::Incremental {
        return Err(BackupError::Other(
            "this is a full backup; restore it into an empty folder instead".into(),
        ));
    }
    let marker = archive_root.join(ARCHIVE_FILE);
    let meta: ArchiveMeta =
        serde_json::from_slice(&fs::read(&marker).map_err(BackupError::io(&marker))?)
            .map_err(|e| BackupError::Manifest(e.to_string()))?;
    if meta.archive_id != manifest.archive_id {
        return Err(BackupError::WrongArchive);
    }
    let staging = archive_root.join(STAGING_DIR);
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(BackupError::io(&staging))?;
    let (files, bytes) = match extract_verified(zip_path, &manifest, &staging, ctl) {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    let stamp = chrono::Local::now()
        .format("%Y%m%d-%H%M%S-incremental")
        .to_string();
    let trash = archive_root.join(TRASH_DIR).join(stamp);
    // Replace changed files.
    for f in &manifest.files {
        let rel = check_entry_name(&f.path[ARCHIVE_PREFIX.len()..])?;
        let from = staging.join(&rel);
        let to = archive_root.join(&rel);
        if to.exists() {
            fsutil::move_path(&to, &trash.join(&rel)).map_err(BackupError::io(&to))?;
        }
        fsutil::move_path(&from, &to).map_err(BackupError::io(&to))?;
    }
    // Files deleted since the previous backup go to the trash.
    let keep: HashSet<String> = manifest
        .snapshot
        .iter()
        .map(|f| f.path[ARCHIVE_PREFIX.len()..].to_owned())
        .collect();
    for entry in walkdir::WalkDir::new(archive_root)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0 || !fsutil::is_hidden_name(&e.file_name().to_string_lossy())
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let Ok(rel) = entry.path().strip_prefix(archive_root) else {
            continue;
        };
        let rel_s = fsutil::to_slash(rel);
        if !keep.contains(&rel_s) {
            let _ = fsutil::move_path(entry.path(), &trash.join(rel));
        }
    }
    let _ = fs::remove_dir_all(&staging);
    Ok(RestoreReport {
        root: archive_root.to_path_buf(),
        files,
        bytes,
        collections: manifest.stats.collections,
        photos: manifest.stats.photos,
    })
}
