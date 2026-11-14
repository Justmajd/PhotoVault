//! The archive: a folder on disk that *is* the database.
//!
//! [`Archive`] keeps an in-memory index of every collection, built by
//! scanning the folder. All mutations write straight back to the folder
//! (atomically), so the in-memory index is only ever a cache.

pub mod import;
pub mod integrity;
pub mod model;
pub mod naming;
pub mod ops;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use thiserror::Error;
use uuid::Uuid;

use crate::fsutil;
use crate::progress::JobCtl;
use import::{DupLocation, ImportRequest};
use model::{
    ARCHIVE_FILE, ARCHIVE_FORMAT, ArchiveMeta, COLLECTION_FILE, Collection, FORMAT_VERSION,
    NOTES_FILE,
};

pub use ops::{TrashBatch, TrashKind};

/// Errors from archive operations.
#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("{path}: invalid JSON: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{0} is not a PhotoVault archive (no photovault.json)")]
    NotAnArchive(PathBuf),
    #[error(
        "this archive was created by a newer PhotoVault (format version {0}); please update the app"
    )]
    UnsupportedVersion(u32),
    #[error("{0} already contains a PhotoVault archive")]
    AlreadyExists(PathBuf),
    #[error("collection not found")]
    CollectionNotFound(Uuid),
    #[error("{0}")]
    Other(String),
}

impl ArchiveError {
    pub(crate) fn io(path: &Path) -> impl FnOnce(io::Error) -> ArchiveError + '_ {
        move |source| ArchiveError::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// Result alias for archive operations.
pub type Result<T> = std::result::Result<T, ArchiveError>;

/// A collection plus its location and runtime-only state.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectionEntry {
    /// Serialized data (`collection.json`).
    pub data: Collection,
    /// Folder relative to the archive root, e.g. `2025/2025-08-02 - Trip`.
    pub rel_dir: PathBuf,
    /// Listed photos whose files are missing on disk.
    pub missing: HashSet<String>,
    /// A folder rename failed and should be retried on the next save.
    pub rename_pending: bool,
}

impl CollectionEntry {
    /// Folder name (last path component).
    pub fn folder_name(&self) -> String {
        self.rel_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// What a scan found and fixed.
#[derive(Clone, Debug, Default)]
pub struct ScanReport {
    /// Orphan files that were adopted: `(collection title, file name)`.
    pub adopted_orphans: Vec<(String, String)>,
    /// Number of listed files that are missing.
    pub missing: usize,
    /// Folders containing media but no `collection.json` (relative paths).
    pub unadopted: Vec<PathBuf>,
    /// Leftover temp files that were deleted.
    pub cleaned_temp: usize,
    /// Non-fatal problems (unreadable JSON, …).
    pub errors: Vec<String>,
}

/// What kind of folder the user pointed at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderKind {
    /// Has `photovault.json`.
    Archive,
    /// No marker, but contains `YYYY/…` folders with media or collection files.
    Adoptable { folders: usize },
    /// Does not exist or is empty.
    Empty,
    /// Something else (non-empty, no recognizable layout).
    Other,
}

/// An open archive.
#[derive(Debug)]
pub struct Archive {
    pub root: PathBuf,
    pub meta: ArchiveMeta,
    /// Collections sorted newest first.
    pub collections: Vec<CollectionEntry>,
    /// Result of the most recent scan.
    pub last_scan: ScanReport,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(ArchiveError::io(path))?;
    serde_json::from_slice(&bytes).map_err(|source| ArchiveError::Json {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| ArchiveError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    bytes.push(b'\n');
    fsutil::atomic_write(path, &bytes).map_err(ArchiveError::io(path))
}

fn is_year_name(name: &str) -> bool {
    name.len() == 4 && name.chars().all(|c| c.is_ascii_digit())
}

fn list_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(n, _)| !fsutil::is_hidden_name(n))
        .collect();
    out.sort();
    out
}

/// Media files directly inside `dir` (not recursive), sorted by name.
pub fn media_files_in(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !fsutil::is_hidden_name(n) && import::is_media(Path::new(n)))
        .collect();
    out.sort();
    out
}

/// Removes leftover `*.pvtmp` files in `dir`. Returns how many were removed.
fn clean_temp_files(dir: &Path) -> usize {
    let mut n = 0;
    for entry in fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if fsutil::is_temp_name(&name) && fs::remove_file(entry.path()).is_ok() {
            n += 1;
        }
    }
    n
}

impl Archive {
    /// Classifies a folder before opening it.
    pub fn inspect_folder(root: &Path) -> FolderKind {
        if root.join(ARCHIVE_FILE).is_file() {
            return FolderKind::Archive;
        }
        let Ok(mut rd) = fs::read_dir(root) else {
            return FolderKind::Empty;
        };
        if rd.next().is_none() {
            return FolderKind::Empty;
        }
        let mut folders = 0;
        for (year, year_path) in list_dirs(root) {
            if !is_year_name(&year) {
                continue;
            }
            for (_, dir) in list_dirs(&year_path) {
                if dir.join(COLLECTION_FILE).is_file() || !media_files_in(&dir).is_empty() {
                    folders += 1;
                }
            }
        }
        if folders > 0 {
            FolderKind::Adoptable { folders }
        } else {
            FolderKind::Other
        }
    }

    /// Creates a new archive in `root` (which may exist but must not already be an archive).
    pub fn create(root: &Path, name: &str) -> Result<Archive> {
        if root.join(ARCHIVE_FILE).exists() {
            return Err(ArchiveError::AlreadyExists(root.to_path_buf()));
        }
        fs::create_dir_all(root).map_err(ArchiveError::io(root))?;
        let meta = ArchiveMeta::new(name);
        write_json(&root.join(ARCHIVE_FILE), &meta)?;
        let mut archive = Archive {
            root: root.to_path_buf(),
            meta,
            collections: Vec::new(),
            last_scan: ScanReport::default(),
        };
        archive.scan(None)?;
        Ok(archive)
    }

    /// Writes a `photovault.json` into an existing folder of collections and
    /// adopts every folder that has media but no `collection.json`.
    pub fn adopt_existing(root: &Path, name: &str, ctl: Option<&JobCtl>) -> Result<Archive> {
        let mut archive = Self::create(root, name)?;
        let pending = archive.last_scan.unadopted.clone();
        for rel in pending {
            let entry = Self::adopt_folder(&archive.root, &rel, ctl)?;
            archive.insert_entry(entry);
        }
        archive.last_scan.unadopted.clear();
        Ok(archive)
    }

    /// Opens and scans an existing archive.
    pub fn open(root: &Path, ctl: Option<&JobCtl>) -> Result<Archive> {
        let marker = root.join(ARCHIVE_FILE);
        if !marker.is_file() {
            return Err(ArchiveError::NotAnArchive(root.to_path_buf()));
        }
        let meta: ArchiveMeta = read_json(&marker)?;
        if meta.format != ARCHIVE_FORMAT {
            return Err(ArchiveError::NotAnArchive(root.to_path_buf()));
        }
        if meta.format_version > FORMAT_VERSION {
            return Err(ArchiveError::UnsupportedVersion(meta.format_version));
        }
        let mut archive = Archive {
            root: root.to_path_buf(),
            meta,
            collections: Vec::new(),
            last_scan: ScanReport::default(),
        };
        archive.scan(ctl)?;
        Ok(archive)
    }

    /// Re-reads every collection from disk, cleans temp files, adopts
    /// orphans and flags missing files.
    pub fn scan(&mut self, ctl: Option<&JobCtl>) -> Result<&ScanReport> {
        let mut report = ScanReport {
            cleaned_temp: clean_temp_files(&self.root),
            ..Default::default()
        };
        let mut collections = Vec::new();
        let mut seen_ids = HashSet::new();
        let years: Vec<_> = list_dirs(&self.root)
            .into_iter()
            .filter(|(n, _)| is_year_name(n))
            .collect();
        if let Some(ctl) = ctl {
            ctl.begin_phase("Scanning", years.len() as u64, 0);
        }
        for (year, year_path) in years {
            if let Some(ctl) = ctl {
                ctl.set_current(&year);
            }
            report.cleaned_temp += clean_temp_files(&year_path);
            for (folder, dir) in list_dirs(&year_path) {
                report.cleaned_temp += clean_temp_files(&dir);
                let rel_dir = PathBuf::from(&year).join(&folder);
                let json_path = dir.join(COLLECTION_FILE);
                if !json_path.is_file() {
                    if !media_files_in(&dir).is_empty() {
                        report.unadopted.push(rel_dir);
                    }
                    continue;
                }
                let mut data: Collection = match read_json(&json_path) {
                    Ok(c) => c,
                    Err(e) => {
                        report.errors.push(e.to_string());
                        continue;
                    }
                };
                let mut needs_save = false;
                if !seen_ids.insert(data.id) {
                    // A copied folder: give it its own identity.
                    data.id = Uuid::new_v4();
                    seen_ids.insert(data.id);
                    needs_save = true;
                }
                // Orphans: media in the folder that the JSON does not list.
                let listed: HashSet<String> =
                    data.photos.iter().map(|p| p.file_name.clone()).collect();
                for file in media_files_in(&dir) {
                    if listed.contains(&file) {
                        continue;
                    }
                    let path = dir.join(&file);
                    match fsutil::sha256_file(&path, None) {
                        Ok(sha) => {
                            data.photos
                                .push(import::photo_from_file(&path, &sha, &file));
                            report.adopted_orphans.push((data.title.clone(), file));
                            needs_save = true;
                        }
                        Err(e) => report.errors.push(format!("{}: {e}", path.display())),
                    }
                }
                let missing: HashSet<String> = data
                    .photos
                    .iter()
                    .filter(|p| !dir.join(&p.file_name).is_file())
                    .map(|p| p.file_name.clone())
                    .collect();
                report.missing += missing.len();
                let entry = CollectionEntry {
                    data,
                    rel_dir,
                    missing,
                    rename_pending: false,
                };
                if needs_save && let Err(e) = ops::write_collection_files(&self.root, &entry) {
                    report.errors.push(e.to_string());
                }
                collections.push(entry);
            }
            if let Some(ctl) = ctl {
                ctl.inc_done();
            }
        }
        self.collections = collections;
        self.sort();
        self.last_scan = report;
        Ok(&self.last_scan)
    }

    /// Sorts collections newest first, then by title.
    pub fn sort(&mut self) {
        self.collections.sort_by(|a, b| {
            b.data.date.cmp(&a.data.date).then_with(|| {
                a.data
                    .title
                    .to_lowercase()
                    .cmp(&b.data.title.to_lowercase())
            })
        });
    }

    /// Builds a collection from a folder that has media but no `collection.json`.
    /// Date and title are parsed from the folder name when possible.
    pub fn adopt_folder(
        root: &Path,
        rel_dir: &Path,
        ctl: Option<&JobCtl>,
    ) -> Result<CollectionEntry> {
        let dir = root.join(rel_dir);
        let folder = rel_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let files = media_files_in(&dir);
        let (date, title) = match naming::parse_folder_name(&folder) {
            Some((d, t)) => (d, if t.is_empty() { folder.clone() } else { t }),
            None => {
                let paths: Vec<PathBuf> = files.iter().map(|f| dir.join(f)).collect();
                let date = import::suggest_date(&paths, None)
                    .map(|s| s.date)
                    .unwrap_or_else(|| chrono::Local::now().date_naive());
                (date, folder.clone())
            }
        };
        let mut data = Collection::new(&title, date);
        if let Some(ctl) = ctl {
            ctl.begin_phase("Adopting", files.len() as u64, 0);
        }
        for file in &files {
            let path = dir.join(file);
            if let Some(ctl) = ctl {
                ctl.set_current(file);
            }
            let sha = fsutil::sha256_file(&path, ctl).map_err(ArchiveError::io(&path))?;
            data.photos.push(import::photo_from_file(&path, &sha, file));
            if let Some(ctl) = ctl {
                ctl.inc_done();
            }
        }
        let entry = CollectionEntry {
            data,
            rel_dir: rel_dir.to_path_buf(),
            missing: HashSet::new(),
            rename_pending: false,
        };
        ops::write_collection_files(root, &entry)?;
        Ok(entry)
    }

    /// Adds an entry to the index (keeping sort order).
    pub fn insert_entry(&mut self, entry: CollectionEntry) {
        self.collections.retain(|c| c.data.id != entry.data.id);
        self.collections.push(entry);
        self.sort();
    }

    /// Saves `photovault.json`.
    pub fn save_meta(&self) -> Result<()> {
        write_json(&self.root.join(ARCHIVE_FILE), &self.meta)
    }

    /// Looks up a collection.
    pub fn get(&self, id: Uuid) -> Option<&CollectionEntry> {
        self.collections.iter().find(|c| c.data.id == id)
    }

    /// Looks up a collection mutably.
    pub fn get_mut(&mut self, id: Uuid) -> Option<&mut CollectionEntry> {
        self.collections.iter_mut().find(|c| c.data.id == id)
    }

    pub(crate) fn entry_mut(&mut self, id: Uuid) -> Result<&mut CollectionEntry> {
        self.get_mut(id).ok_or(ArchiveError::CollectionNotFound(id))
    }

    pub(crate) fn entry(&self, id: Uuid) -> Result<&CollectionEntry> {
        self.get(id).ok_or(ArchiveError::CollectionNotFound(id))
    }

    /// Absolute folder of a collection.
    pub fn collection_dir(&self, id: Uuid) -> Option<PathBuf> {
        self.get(id).map(|c| self.root.join(&c.rel_dir))
    }

    /// Absolute path of a photo.
    pub fn photo_path(&self, id: Uuid, file_name: &str) -> Option<PathBuf> {
        self.collection_dir(id).map(|d| d.join(file_name))
    }

    /// Creates a new, empty collection folder.
    pub fn create_collection(&mut self, title: &str, date: NaiveDate) -> Result<Uuid> {
        let data = Collection::new(title.trim(), date);
        let year_dir = self.root.join(naming::year_folder_name(date));
        fs::create_dir_all(&year_dir).map_err(ArchiveError::io(&year_dir))?;
        let name = naming::unique_name(
            &year_dir,
            &naming::collection_folder_name(date, title),
            &HashSet::new(),
            false,
        );
        let dir = year_dir.join(&name);
        fs::create_dir(&dir).map_err(ArchiveError::io(&dir))?;
        let id = data.id;
        let entry = CollectionEntry {
            data,
            rel_dir: PathBuf::from(naming::year_folder_name(date)).join(name),
            missing: HashSet::new(),
            rename_pending: false,
        };
        ops::write_collection_files(&self.root, &entry)?;
        self.insert_entry(entry);
        Ok(id)
    }

    /// Total number of photos.
    pub fn photo_count(&self) -> usize {
        self.collections.iter().map(|c| c.data.photos.len()).sum()
    }

    /// Total bytes of all photos.
    pub fn total_bytes(&self) -> u64 {
        self.collections.iter().map(|c| c.data.total_bytes()).sum()
    }

    /// Every known tag, sorted, with usage counts.
    pub fn all_tags(&self) -> Vec<(String, usize)> {
        let mut map: HashMap<String, usize> = HashMap::new();
        for c in &self.collections {
            for t in &c.data.tags {
                *map.entry(t.clone()).or_default() += 1;
            }
        }
        let mut v: Vec<_> = map.into_iter().collect();
        v.sort_by_key(|a| a.0.to_lowercase());
        v
    }

    /// SHA-256 → location for every photo in the archive.
    pub fn hash_index(&self) -> HashMap<String, DupLocation> {
        let mut map = HashMap::new();
        for c in &self.collections {
            for p in &c.data.photos {
                map.entry(p.sha256.clone()).or_insert_with(|| DupLocation {
                    collection_id: c.data.id,
                    collection_title: c.data.title.clone(),
                    file_name: p.file_name.clone(),
                });
            }
        }
        map
    }

    /// Captures everything an import job needs for collection `id`.
    pub fn import_request(&self, id: Uuid, files: Vec<PathBuf>) -> Result<ImportRequest> {
        let entry = self.entry(id)?;
        let dir = self.root.join(&entry.rel_dir);
        let mut taken: HashSet<String> = entry
            .data
            .photos
            .iter()
            .map(|p| p.file_name.to_lowercase())
            .collect();
        for f in media_files_in(&dir) {
            taken.insert(f.to_lowercase());
        }
        taken.insert(COLLECTION_FILE.to_lowercase());
        taken.insert(NOTES_FILE.to_lowercase());
        let hashes_here = entry.data.photos.iter().map(|p| p.sha256.clone()).collect();
        let mut elsewhere = self.hash_index();
        elsewhere.retain(|_, loc| loc.collection_id != id);
        Ok(ImportRequest {
            collection_dir: dir,
            collection_id: id,
            files,
            taken_names: taken,
            hashes_here,
            hashes_elsewhere: elsewhere,
        })
    }

    /// Path of the trash folder.
    pub fn trash_dir(&self) -> PathBuf {
        self.root.join(model::TRASH_DIR)
    }

    /// Size of the trash in bytes.
    pub fn trash_size(&self) -> u64 {
        fsutil::dir_size(&self.trash_dir())
    }

    /// Permanently deletes everything in the trash.
    pub fn empty_trash(&self) -> Result<()> {
        let dir = self.trash_dir();
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(ArchiveError::io(&dir))?;
        }
        Ok(())
    }
}
