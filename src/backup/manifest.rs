//! `manifest.json` stored inside every backup zip.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `format` value of a backup manifest.
pub const BACKUP_FORMAT: &str = "photovault-backup";
/// Current major version of the backup format.
pub const BACKUP_FORMAT_VERSION: u32 = 1;
/// Name of the manifest entry in the zip.
pub const MANIFEST_NAME: &str = "manifest.json";
/// Name of the readme entry in the zip.
pub const README_NAME: &str = "README.txt";
/// Prefix of all archive entries in the zip.
pub const ARCHIVE_PREFIX: &str = "archive/";

/// Full or incremental backup.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackupKind {
    #[default]
    Full,
    Incremental,
}

/// Summary numbers.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BackupStats {
    pub collections: usize,
    pub photos: usize,
    pub total_bytes: u64,
}

/// One file in the backup.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ManifestFile {
    /// Path inside the zip, always `/`-separated, starting with `archive/`.
    pub path: String,
    pub size: u64,
    pub sha256: String,
    /// Modified time, Unix seconds.
    pub mtime: i64,
}

/// Contents of `manifest.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub app_version: String,
    pub created_at: DateTime<Utc>,
    pub source_os: String,
    pub archive_id: Uuid,
    pub archive_name: String,
    pub stats: BackupStats,
    /// Files contained in this zip.
    pub files: Vec<ManifestFile>,
    #[serde(default)]
    pub kind: BackupKind,
    /// For incremental backups: `created_at` of the previous backup in the chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_created_at: Option<DateTime<Utc>>,
    /// For incremental backups: every file of the archive at backup time
    /// (including unchanged ones not stored in this zip), used to apply deletions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snapshot: Vec<ManifestFile>,
}

impl Manifest {
    /// Full state of the archive at backup time.
    pub fn state(&self) -> &[ManifestFile] {
        if self.kind == BackupKind::Incremental {
            &self.snapshot
        } else {
            &self.files
        }
    }
}

/// `windows`, `macos` or `linux`.
pub fn current_os() -> &'static str {
    std::env::consts::OS
}

/// The plain-English README stored in every backup.
pub fn readme_text(m: &Manifest) -> String {
    format!(
        "PhotoVault backup\n\
=================\n\
\n\
Archive:   {name}\n\
Created:   {created}\n\
Type:      {kind}\n\
Contents:  {collections} collections, {photos} photos\n\
\n\
This is a normal .zip file. You do NOT need PhotoVault to get your photos back.\n\
\n\
What is inside\n\
--------------\n\
  README.txt      this file\n\
  manifest.json   list of every file with its size and SHA-256 checksum\n\
  archive/        an exact copy of the PhotoVault archive folder\n\
\n\
Inside archive/ every collection is a folder named\n\
\n\
    archive/YYYY/YYYY-MM-DD - Title/\n\
\n\
containing the original photo files, a collection.json file (title, date,\n\
notes, captions) and NOTES.md (the same information as readable text).\n\
\n\
Recovering your photos without PhotoVault\n\
-----------------------------------------\n\
1. Unzip this file with any tool (Windows Explorer, macOS Finder, unzip).\n\
2. Open the archive/ folder. Your photos are in archive/YYYY/...\n\
3. Read NOTES.md in each folder for the notes you wrote.\n\
\n\
Restoring with PhotoVault\n\
-------------------------\n\
Open PhotoVault, choose \"Restore\", pick this .zip and an empty destination\n\
folder. Every file is checked against manifest.json before it is put in place.\n\
Alternatively unzip it yourself and choose \"Open existing archive\" on the\n\
extracted archive/ folder.\n\
{incremental}",
        name = m.archive_name,
        created = m.created_at.format("%Y-%m-%d %H:%M UTC"),
        kind = if m.kind == BackupKind::Incremental {
            "incremental (changes only)"
        } else {
            "full"
        },
        collections = m.stats.collections,
        photos = m.stats.photos,
        incremental = if m.kind == BackupKind::Incremental {
            "\nThis is an INCREMENTAL backup: it only contains files that changed since the\n\
previous backup. Restore the full backup first, then apply each incremental\n\
backup in order (oldest first).\n"
        } else {
            ""
        },
    )
}
