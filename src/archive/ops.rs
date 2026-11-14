//! Mutating operations: atomic saves, folder renames, trash and undo, moves.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::model::{COLLECTION_FILE, Collection, NOTES_FILE, Photo, TRASH_DIR, now};
use super::{Archive, ArchiveError, CollectionEntry, Result, naming, write_json};
use crate::fsutil;

/// Writes `collection.json` and the `NOTES.md` mirror atomically.
pub fn write_collection_files(root: &Path, entry: &CollectionEntry) -> Result<()> {
    let dir = root.join(&entry.rel_dir);
    write_json(&dir.join(COLLECTION_FILE), &entry.data)?;
    let notes = dir.join(NOTES_FILE);
    fsutil::atomic_write(&notes, entry.data.render_notes_md().as_bytes())
        .map_err(ArchiveError::io(&notes))
}

/// What happened when saving a collection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveOutcome {
    /// The folder was renamed/moved to match title and date.
    pub renamed: bool,
    /// The rename was attempted and failed (metadata was still saved).
    pub rename_error: Option<String>,
}

/// Something that was moved to the trash and can be restored.
#[derive(Clone, Debug)]
pub struct TrashBatch {
    /// Trash sub-folder name (timestamp).
    pub id: String,
    pub kind: TrashKind,
}

/// The contents of a [`TrashBatch`].
#[derive(Clone, Debug)]
pub enum TrashKind {
    /// Photos removed from a collection, with their original positions.
    Photos {
        collection_id: Uuid,
        removed: Vec<(usize, Photo)>,
        /// `(original absolute path, path in trash)`.
        files: Vec<(PathBuf, PathBuf)>,
        previous_cover: Option<String>,
    },
    /// A whole collection folder.
    Collection {
        data: Box<Collection>,
        rel_dir: PathBuf,
        trash_path: PathBuf,
    },
}

fn trash_stamp() -> String {
    format!(
        "{}-{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        &Uuid::new_v4().simple().to_string()[..6]
    )
}

impl Archive {
    /// Saves a collection's metadata (always) and, if `sync_folder` is set or a
    /// previous rename failed, renames its folder to match title and date.
    pub fn save_collection(&mut self, id: Uuid, sync_folder: bool) -> Result<SaveOutcome> {
        let root = self.root.clone();
        let entry = self.entry_mut(id)?;
        entry.data.updated_at = now();
        write_collection_files(&root, entry)?;
        let mut outcome = SaveOutcome::default();
        if sync_folder || entry.rename_pending {
            match self.sync_folder(id) {
                Ok(renamed) => outcome.renamed = renamed,
                Err(e) => {
                    if let Ok(entry) = self.entry_mut(id) {
                        entry.rename_pending = true;
                    }
                    outcome.rename_error = Some(e.to_string());
                }
            }
        }
        Ok(outcome)
    }

    /// Renames/moves the collection folder to `YYYY/YYYY-MM-DD - Title`.
    /// Returns `true` if the folder moved.
    pub fn sync_folder(&mut self, id: Uuid) -> Result<bool> {
        let root = self.root.clone();
        let entry = self.entry_mut(id)?;
        let date = entry.data.date;
        let current_name = entry.folder_name();
        let current_year = entry
            .rel_dir
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let target_year = naming::year_folder_name(date);
        let canonical = naming::collection_folder_name(date, &entry.data.title);
        if current_year == target_year
            && naming::folder_matches(&current_name, date, &entry.data.title)
        {
            entry.rename_pending = false;
            return Ok(false);
        }
        let year_dir = root.join(&target_year);
        fs::create_dir_all(&year_dir).map_err(ArchiveError::io(&year_dir))?;
        // A case-only rename on a case-insensitive FS must not collide with itself.
        let new_name = if current_year == target_year
            && current_name.to_lowercase() == canonical.to_lowercase()
        {
            canonical
        } else {
            naming::unique_name(&year_dir, &canonical, &HashSet::new(), false)
        };
        let old_abs = root.join(&entry.rel_dir);
        let new_rel = PathBuf::from(&target_year).join(&new_name);
        let new_abs = root.join(&new_rel);
        fs::rename(&old_abs, &new_abs).map_err(ArchiveError::io(&old_abs))?;
        entry.rel_dir = new_rel;
        entry.rename_pending = false;
        // Remove the old year folder if it became empty.
        if current_year != target_year {
            let old_year = root.join(&current_year);
            if fs::read_dir(&old_year)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
            {
                let _ = fs::remove_dir(&old_year);
            }
        }
        fsutil::sync_dir(&year_dir);
        self.sort();
        Ok(true)
    }

    /// Appends imported photos and saves.
    pub fn add_photos(&mut self, id: Uuid, photos: Vec<Photo>) -> Result<()> {
        let entry = self.entry_mut(id)?;
        for p in photos {
            entry.missing.remove(&p.file_name);
            if entry.data.photo_index(&p.file_name).is_none() {
                entry.data.photos.push(p);
            }
        }
        self.save_collection(id, false)?;
        Ok(())
    }

    fn trash_root(&self, stamp: &str) -> PathBuf {
        self.root.join(TRASH_DIR).join(stamp)
    }

    /// Moves photos to the trash and removes them from the collection.
    pub fn remove_photos(&mut self, id: Uuid, names: &[String]) -> Result<TrashBatch> {
        let stamp = trash_stamp();
        let trash = self.trash_root(&stamp);
        let root = self.root.clone();
        let entry = self.entry_mut(id)?;
        let dir = root.join(&entry.rel_dir);
        let previous_cover = entry.data.cover_photo.clone();
        let wanted: HashSet<&String> = names.iter().collect();
        let mut removed = Vec::new();
        let mut files = Vec::new();
        let mut kept = Vec::with_capacity(entry.data.photos.len());
        for (i, p) in std::mem::take(&mut entry.data.photos)
            .into_iter()
            .enumerate()
        {
            if wanted.contains(&p.file_name) {
                let src = dir.join(&p.file_name);
                if src.exists() {
                    let dst = trash.join(&entry.rel_dir).join(&p.file_name);
                    if let Err(e) = fsutil::move_path(&src, &dst) {
                        // Put everything back the way it was and stop.
                        kept.push(p);
                        log::error!("failed to trash {}: {e}", src.display());
                        continue;
                    }
                    files.push((src, dst));
                }
                entry.missing.remove(&p.file_name);
                removed.push((i, p));
            } else {
                kept.push(p);
            }
        }
        entry.data.photos = kept;
        if let Some(c) = &entry.data.cover_photo
            && wanted.contains(c)
        {
            entry.data.cover_photo = None;
        }
        self.save_collection(id, false)?;
        Ok(TrashBatch {
            id: stamp,
            kind: TrashKind::Photos {
                collection_id: id,
                removed,
                files,
                previous_cover,
            },
        })
    }

    /// Moves a whole collection folder to the trash.
    pub fn delete_collection(&mut self, id: Uuid) -> Result<TrashBatch> {
        let stamp = trash_stamp();
        let trash = self.trash_root(&stamp);
        let entry = self.entry(id)?.clone();
        let src = self.root.join(&entry.rel_dir);
        let dst = trash.join(&entry.rel_dir);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(ArchiveError::io(parent))?;
        }
        fs::rename(&src, &dst).map_err(ArchiveError::io(&src))?;
        if let Some(year) = src.parent()
            && fs::read_dir(year)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
        {
            let _ = fs::remove_dir(year);
        }
        self.collections.retain(|c| c.data.id != id);
        Ok(TrashBatch {
            id: stamp,
            kind: TrashKind::Collection {
                data: Box::new(entry.data),
                rel_dir: entry.rel_dir,
                trash_path: dst,
            },
        })
    }

    /// Restores a trash batch.
    pub fn undo_trash(&mut self, batch: TrashBatch) -> Result<()> {
        match batch.kind {
            TrashKind::Photos {
                collection_id,
                removed,
                files,
                previous_cover,
            } => {
                for (orig, trashed) in &files {
                    if trashed.exists() {
                        fsutil::move_path(trashed, orig).map_err(ArchiveError::io(trashed))?;
                    }
                }
                let entry = self.entry_mut(collection_id)?;
                for (i, p) in removed {
                    let idx = i.min(entry.data.photos.len());
                    entry.data.photos.insert(idx, p);
                }
                entry.data.cover_photo = previous_cover;
                self.save_collection(collection_id, false)?;
            }
            TrashKind::Collection {
                data,
                rel_dir,
                trash_path,
            } => {
                let mut target = self.root.join(&rel_dir);
                if target.exists() {
                    let parent = target
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| self.root.clone());
                    let base = rel_dir
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    target =
                        parent.join(naming::unique_name(&parent, &base, &HashSet::new(), false));
                }
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(ArchiveError::io(parent))?;
                }
                fs::rename(&trash_path, &target).map_err(ArchiveError::io(&trash_path))?;
                let rel = target
                    .strip_prefix(&self.root)
                    .map(Path::to_path_buf)
                    .unwrap_or(rel_dir);
                let missing = data
                    .photos
                    .iter()
                    .filter(|p| !target.join(&p.file_name).is_file())
                    .map(|p| p.file_name.clone())
                    .collect();
                self.insert_entry(CollectionEntry {
                    data: *data,
                    rel_dir: rel,
                    missing,
                    rename_pending: false,
                });
            }
        }
        let batch_dir = self.trash_root(&batch.id);
        let _ = remove_empty_dirs(&batch_dir);
        Ok(())
    }

    /// Moves photos (files + metadata) from one collection to another.
    pub fn move_photos(&mut self, from: Uuid, to: Uuid, names: &[String]) -> Result<usize> {
        if from == to {
            return Ok(0);
        }
        let from_dir = self.root.join(&self.entry(from)?.rel_dir);
        let to_dir = self.root.join(&self.entry(to)?.rel_dir);
        let mut taken: HashSet<String> = self
            .entry(to)?
            .data
            .photos
            .iter()
            .map(|p| p.file_name.to_lowercase())
            .collect();
        taken.insert(COLLECTION_FILE.to_lowercase());
        taken.insert(NOTES_FILE.to_lowercase());
        let wanted: HashSet<&String> = names.iter().collect();
        let mut moved = Vec::new();
        {
            let src_entry = self.entry_mut(from)?;
            let mut kept = Vec::new();
            for p in std::mem::take(&mut src_entry.data.photos) {
                if !wanted.contains(&p.file_name) {
                    kept.push(p);
                    continue;
                }
                let new_name = naming::unique_name(&to_dir, &p.file_name, &taken, true);
                let src = from_dir.join(&p.file_name);
                let dst = to_dir.join(&new_name);
                if src.exists()
                    && let Err(e) = fsutil::move_path(&src, &dst)
                {
                    log::error!("failed to move {}: {e}", src.display());
                    kept.push(p);
                    continue;
                }
                if src_entry.data.cover_photo.as_deref() == Some(p.file_name.as_str()) {
                    src_entry.data.cover_photo = None;
                }
                src_entry.missing.remove(&p.file_name);
                taken.insert(new_name.to_lowercase());
                let mut p = p;
                p.file_name = new_name;
                moved.push(p);
            }
            src_entry.data.photos = kept;
        }
        let n = moved.len();
        self.entry_mut(to)?.data.photos.extend(moved);
        self.save_collection(from, false)?;
        self.save_collection(to, false)?;
        Ok(n)
    }

    /// Moves a photo to a new position in the display order.
    pub fn reorder_photo(&mut self, id: Uuid, file_name: &str, new_index: usize) -> Result<()> {
        let entry = self.entry_mut(id)?;
        if let Some(i) = entry.data.photo_index(file_name) {
            let p = entry.data.photos.remove(i);
            let idx = new_index.min(entry.data.photos.len());
            entry.data.photos.insert(idx, p);
        }
        self.save_collection(id, false)?;
        Ok(())
    }
}

fn remove_empty_dirs(dir: &Path) -> std::io::Result<bool> {
    if !dir.is_dir() {
        return Ok(true);
    }
    let mut empty = true;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if !remove_empty_dirs(&entry.path())? {
                empty = false;
            }
        } else {
            empty = false;
        }
    }
    if empty {
        fs::remove_dir(dir)?;
    }
    Ok(empty)
}
