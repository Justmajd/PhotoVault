//! App configuration (`<config_dir>/PhotoVault/config.json`).
//!
//! The config is a convenience only: it remembers recent archives and UI
//! preferences, and never holds archive data.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Theme preference.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

/// UI language.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En = 0,
    Ar = 1,
}

/// Persistent app preferences.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    pub last_archive: Option<PathBuf>,
    pub recent_archives: Vec<PathBuf>,
    pub theme: ThemeMode,
    /// Index into the accent preset list.
    pub accent: usize,
    /// Thumbnail edge length in points.
    pub thumb_size: f32,
    /// Backup reminder interval in days (0 = off).
    pub backup_reminder_days: u32,
    pub confirm_before_delete: bool,
    pub window_size: Option<[f32; 2]>,
    pub sidebar_width: f32,
    pub language: Language,
    /// Slideshow interval in seconds.
    pub slideshow_secs: f32,
    /// Folder of the most recent backup.
    pub last_backup_dir: Option<PathBuf>,
    /// UI zoom factor.
    pub ui_scale: f32,
    /// Smooth transitions and entrance animations.
    pub animations: bool,
    /// Check for updates automatically at startup.
    pub check_updates: bool,
    /// Unix timestamp of the last update check (manual or automatic).
    pub last_update_check: Option<i64>,
    /// The first-run introduction was shown.
    pub intro_seen: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            last_archive: None,
            recent_archives: Vec::new(),
            theme: ThemeMode::Dark,
            accent: 0,
            thumb_size: 150.0,
            backup_reminder_days: 30,
            confirm_before_delete: true,
            window_size: None,
            sidebar_width: 300.0,
            language: Language::En,
            slideshow_secs: 4.0,
            last_backup_dir: None,
            ui_scale: 1.0,
            animations: true,
            check_updates: true,
            last_update_check: None,
            intro_seen: false,
        }
    }
}

/// `<config_dir>/PhotoVault`.
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("PhotoVault")
}

/// Path of `config.json`.
pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Path of the log file.
pub fn log_path() -> PathBuf {
    config_dir().join("photovault.log")
}

/// Default location for a new archive: `~/Pictures/PhotoVault Archive`.
pub fn default_archive_path() -> PathBuf {
    dirs::picture_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("PhotoVault Archive")
}

impl AppConfig {
    /// Loads the config, falling back to defaults on any error.
    pub fn load() -> Self {
        Self::load_from(&config_path())
    }

    /// Loads from a specific file.
    pub fn load_from(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Saves atomically.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&config_path())
    }

    /// Saves to a specific file.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::fsutil::atomic_write(path, &bytes)
    }

    /// Records an archive as most recently used.
    pub fn remember_archive(&mut self, root: &Path) {
        self.recent_archives.retain(|p| p != root);
        self.recent_archives.insert(0, root.to_path_buf());
        self.recent_archives.truncate(8);
        self.last_archive = Some(root.to_path_buf());
    }
}
