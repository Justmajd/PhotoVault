//! Serialized data model: `photovault.json` and `collection.json`.
//!
//! Every struct keeps unknown fields in `extra` so that a file written by a
//! newer PhotoVault survives being edited by an older one.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `format` value of `photovault.json`.
pub const ARCHIVE_FORMAT: &str = "photovault-archive";
/// Current major format version for `photovault.json` and `collection.json`.
pub const FORMAT_VERSION: u32 = 1;
/// File name of the archive marker.
pub const ARCHIVE_FILE: &str = "photovault.json";
/// File name of a collection's metadata.
pub const COLLECTION_FILE: &str = "collection.json";
/// File name of the generated human-readable notes mirror.
pub const NOTES_FILE: &str = "NOTES.md";
/// Trash folder inside the archive root.
pub const TRASH_DIR: &str = ".trash";
/// Folder for app state that is not part of the archive (excluded from backups).
pub const STATE_DIR: &str = ".photovault-state";

type Extra = BTreeMap<String, serde_json::Value>;

/// Contents of `photovault.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ArchiveMeta {
    pub format: String,
    pub format_version: u32,
    pub archive_id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub app_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_backup_at: Option<DateTime<Utc>>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl ArchiveMeta {
    /// A fresh archive marker.
    pub fn new(name: &str) -> Self {
        Self {
            format: ARCHIVE_FORMAT.to_owned(),
            format_version: FORMAT_VERSION,
            archive_id: Uuid::new_v4(),
            name: name.to_owned(),
            created_at: now(),
            app_version: crate::APP_VERSION.to_owned(),
            last_backup_at: None,
            extra: Extra::new(),
        }
    }
}

/// GPS position in decimal degrees.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Gps {
    pub lat: f64,
    pub lon: f64,
}

impl Gps {
    /// OpenStreetMap link for this position.
    pub fn osm_url(&self) -> String {
        format!(
            "https://www.openstreetmap.org/?mlat={:.6}&mlon={:.6}#map=15/{:.6}/{:.6}",
            self.lat, self.lon, self.lat, self.lon
        )
    }
}

/// One photo (or other media file) inside a collection.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Photo {
    pub file_name: String,
    #[serde(default)]
    pub original_name: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taken_at: Option<NaiveDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gps: Option<Gps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default = "now")]
    pub added_at: DateTime<Utc>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Photo {
    /// A minimal photo record.
    pub fn new(file_name: &str, sha256: &str, size_bytes: u64) -> Self {
        Self {
            file_name: file_name.to_owned(),
            original_name: file_name.to_owned(),
            sha256: sha256.to_owned(),
            size_bytes,
            taken_at: None,
            camera: None,
            gps: None,
            width: None,
            height: None,
            caption: String::new(),
            favorite: false,
            added_at: now(),
            extra: Extra::new(),
        }
    }
}

/// Contents of `collection.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Collection {
    #[serde(default = "default_version")]
    pub format_version: u32,
    pub id: Uuid,
    #[serde(default)]
    pub title: String,
    pub date: NaiveDate,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_photo: Option<String>,
    #[serde(default = "now")]
    pub created_at: DateTime<Utc>,
    #[serde(default = "now")]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub photos: Vec<Photo>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Collection {
    /// A new empty collection.
    pub fn new(title: &str, date: NaiveDate) -> Self {
        let t = now();
        Self {
            format_version: FORMAT_VERSION,
            id: Uuid::new_v4(),
            title: title.to_owned(),
            date,
            notes: String::new(),
            tags: Vec::new(),
            favorite: false,
            cover_photo: None,
            created_at: t,
            updated_at: t,
            photos: Vec::new(),
            extra: Extra::new(),
        }
    }

    /// Index of a photo by file name.
    pub fn photo_index(&self, file_name: &str) -> Option<usize> {
        self.photos.iter().position(|p| p.file_name == file_name)
    }

    /// The cover photo, falling back to the first photo.
    pub fn cover(&self) -> Option<&Photo> {
        self.cover_photo
            .as_deref()
            .and_then(|c| self.photos.iter().find(|p| p.file_name == c))
            .or_else(|| self.photos.first())
    }

    /// Total bytes of all photos.
    pub fn total_bytes(&self) -> u64 {
        self.photos.iter().map(|p| p.size_bytes).sum()
    }

    /// Renders the `NOTES.md` mirror.
    pub fn render_notes_md(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# {}\n", self.title));
        out.push_str(&format!("Date: {}\n", self.date.format("%Y-%m-%d")));
        if !self.tags.is_empty() {
            out.push_str(&format!("Tags: {}\n", self.tags.join(", ")));
        }
        out.push('\n');
        if !self.notes.is_empty() {
            out.push_str(&self.notes);
            if !self.notes.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        }
        out.push_str("## Photos\n");
        for p in &self.photos {
            if p.caption.trim().is_empty() {
                out.push_str(&format!("- {}\n", p.file_name));
            } else {
                out.push_str(&format!("- {} — {}\n", p.file_name, p.caption.trim()));
            }
        }
        out
    }
}

fn default_version() -> u32 {
    FORMAT_VERSION
}

/// Current UTC time truncated to whole seconds (keeps JSON tidy).
pub fn now() -> DateTime<Utc> {
    let t = Utc::now();
    DateTime::from_timestamp(t.timestamp(), 0).unwrap_or(t)
}
