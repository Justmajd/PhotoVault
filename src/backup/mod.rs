//! Portable `.zip` backups and verified restores.

pub mod create;
pub mod manifest;
pub mod restore;

use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Errors from backup and restore.
#[derive(Debug, Error)]
pub enum BackupError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("invalid manifest: {0}")]
    Manifest(String),
    #[error("this file is not a PhotoVault backup (no valid manifest.json)")]
    NotABackup,
    #[error(
        "this backup was made by a newer PhotoVault (format version {0}); please update the app to restore it"
    )]
    UnsupportedVersion(u32),
    #[error("unsafe path in backup, refusing to extract: {0}")]
    UnsafePath(String),
    #[error("verification failed for {} file(s): {}", .0.len(), .0.join(", "))]
    VerificationFailed(Vec<String>),
    #[error("the destination folder is not empty: {0}")]
    DestinationNotEmpty(PathBuf),
    #[error("this incremental backup belongs to a different archive")]
    WrongArchive,
    #[error("{0}")]
    Other(String),
    #[error("cancelled")]
    Cancelled,
}

impl BackupError {
    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(io::Error) -> BackupError {
        let path = path.into();
        move |source| {
            if source.kind() == io::ErrorKind::Interrupted {
                BackupError::Cancelled
            } else {
                BackupError::Io { path, source }
            }
        }
    }
}

/// Result alias for backup operations.
pub type Result<T> = std::result::Result<T, BackupError>;

pub use create::{BackupOptions, BackupPlan, create_backup, plan_backup, verify_backup};
pub use manifest::{BackupKind, Manifest, ManifestFile};
pub use restore::{
    RestoreReport, apply_incremental, read_manifest, resolve_destination, restore_backup,
};
