//! Thumbnail loader: a small thread pool decodes images off the UI thread
//! (respecting EXIF orientation) into `ColorImage`s, which are uploaded as
//! textures on the UI thread and kept in an LRU cache.
//!
//! Small thumbnails are also cached on disk in `dirs::cache_dir()` keyed by
//! the photo's SHA-256 — never inside the archive.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use image::{DynamicImage, ImageDecoder, ImageReader};

/// Edge length used for grid thumbnails.
pub const THUMB_PX: u32 = 384;
/// Edge length used in the viewer.
pub const LARGE_PX: u32 = 2560;
/// Maximum textures kept alive.
const MAX_TEXTURES: usize = 400;
/// Maximum queued requests (older ones are dropped and re-requested later).
const MAX_QUEUE: usize = 300;

/// Identifies a decoded image.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ThumbKey {
    pub path: PathBuf,
    pub max_px: u32,
}

/// State of a requested image.
pub enum ThumbState<'a> {
    /// The texture and how long ago it finished loading (for fade-ins).
    Ready(&'a TextureHandle, f32),
    Loading,
    Failed,
}

enum Slot {
    Pending,
    Ready(TextureHandle, std::time::Instant),
    Failed,
}

struct Request {
    key: ThumbKey,
    sha: String,
}

type Queue = Arc<(Mutex<Vec<Request>>, Condvar)>;

/// LRU texture cache backed by worker threads.
pub struct ThumbCache {
    slots: HashMap<ThumbKey, (Slot, u64)>,
    frame: u64,
    queue: Queue,
    rx: Receiver<(ThumbKey, Option<ColorImage>)>,
}

fn disk_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("PhotoVault").join("thumbs"))
}

/// Decodes an image file, applies EXIF orientation and downsizes it.
pub fn decode(path: &Path, max_px: u32) -> Option<DynamicImage> {
    let reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).ok()?;
    if img.width() > max_px || img.height() > max_px {
        img = img.thumbnail(max_px, max_px);
    }
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Some(img)
}

fn to_color_image(img: &DynamicImage) -> ColorImage {
    let rgba = img.to_rgba8();
    ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    )
}

fn load(req: &Request) -> Option<ColorImage> {
    let use_disk = req.key.max_px <= THUMB_PX && req.sha.len() == 64;
    let cache_file = disk_cache_dir().filter(|_| use_disk).map(|d| {
        d.join(&req.sha[..2])
            .join(format!("{}_{}.jpg", req.sha, req.key.max_px))
    });
    if let Some(cf) = &cache_file
        && let Ok(img) = image::open(cf)
    {
        return Some(to_color_image(&img));
    }
    let img = decode(&req.key.path, req.key.max_px)?;
    if let Some(cf) = &cache_file
        && let Some(parent) = cf.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        let tmp = cf.with_extension("tmp");
        if img
            .to_rgb8()
            .save_with_format(&tmp, image::ImageFormat::Jpeg)
            .is_ok()
        {
            let _ = std::fs::rename(&tmp, cf);
        }
    }
    Some(to_color_image(&img))
}

impl ThumbCache {
    /// Spawns the worker pool.
    pub fn new(ctx: &egui::Context) -> Self {
        let queue: Queue = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        let (tx, rx): (Sender<(ThumbKey, Option<ColorImage>)>, _) = channel();
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 6);
        for i in 0..workers {
            let queue = queue.clone();
            let tx = tx.clone();
            let ctx = ctx.clone();
            let _ = std::thread::Builder::new()
                .name(format!("thumbs-{i}"))
                .spawn(move || {
                    loop {
                        let req = {
                            let (lock, cvar) = &*queue;
                            let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
                            while q.is_empty() {
                                q = cvar.wait(q).unwrap_or_else(|e| e.into_inner());
                            }
                            // Newest first: what is on screen right now.
                            q.pop()
                        };
                        let Some(req) = req else { continue };
                        let img = load(&req);
                        if tx.send((req.key, img)).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                });
        }
        Self {
            slots: HashMap::new(),
            frame: 0,
            queue,
            rx,
        }
    }

    /// Uploads finished images and evicts old textures. Call once per frame.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        while let Ok((key, img)) = self.rx.try_recv() {
            let Some(slot) = self.slots.get_mut(&key) else {
                continue;
            };
            slot.0 = match img {
                Some(img) => {
                    let name = format!("{}@{}", key.path.display(), key.max_px);
                    Slot::Ready(
                        ctx.load_texture(name, img, TextureOptions::LINEAR),
                        std::time::Instant::now(),
                    )
                }
                None => Slot::Failed,
            };
        }
        let ready = self
            .slots
            .values()
            .filter(|(s, _)| matches!(s, Slot::Ready(..)))
            .count();
        if ready > MAX_TEXTURES {
            let mut by_age: Vec<(u64, ThumbKey)> = self
                .slots
                .iter()
                .filter(|(_, (s, used))| matches!(s, Slot::Ready(..)) && *used + 2 < self.frame)
                .map(|(k, (_, used))| (*used, k.clone()))
                .collect();
            by_age.sort_by_key(|(u, _)| *u);
            for (_, k) in by_age.into_iter().take(ready - MAX_TEXTURES) {
                self.slots.remove(&k);
            }
        }
    }

    fn request(&mut self, key: &ThumbKey, sha: &str) {
        self.slots.insert(key.clone(), (Slot::Pending, self.frame));
        let (lock, cvar) = &*self.queue;
        let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
        q.push(Request {
            key: key.clone(),
            sha: sha.to_owned(),
        });
        if q.len() > MAX_QUEUE {
            let excess = q.len() - MAX_QUEUE;
            let dropped: Vec<Request> = q.drain(..excess).collect();
            for r in dropped {
                self.slots.remove(&r.key);
            }
        }
        cvar.notify_one();
    }

    /// Returns the texture for `key`, requesting it if needed.
    pub fn get(&mut self, key: &ThumbKey, sha: &str) -> ThumbState<'_> {
        if !self.slots.contains_key(key) {
            self.request(key, sha);
        }
        let frame = self.frame;
        match self.slots.get_mut(key) {
            Some((slot, used)) => {
                *used = frame;
                match slot {
                    Slot::Ready(t, at) => ThumbState::Ready(t, at.elapsed().as_secs_f32()),
                    Slot::Pending => ThumbState::Loading,
                    Slot::Failed => ThumbState::Failed,
                }
            }
            None => ThumbState::Loading,
        }
    }

    /// Requests an image without using it this frame (viewer preloading).
    pub fn prefetch(&mut self, key: &ThumbKey, sha: &str) {
        if !self.slots.contains_key(key) {
            self.request(key, sha);
        } else if let Some((_, used)) = self.slots.get_mut(key) {
            *used = self.frame;
        }
    }

    /// Forgets a path (after a file was moved or replaced).
    pub fn forget_dir(&mut self, dir: &Path) {
        self.slots.retain(|k, _| !k.path.starts_with(dir));
    }

    /// Drops everything.
    pub fn clear(&mut self) {
        self.slots.clear();
    }
}
