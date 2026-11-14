//! Archive statistics, "On This Day" and the GPS photo map.

use std::collections::{BTreeMap, HashMap};

use chrono::{Datelike, NaiveDate};
use uuid::Uuid;

use crate::archive::Archive;
use crate::archive::model::Gps;

/// Aggregated numbers for the statistics page.
#[derive(Clone, Debug, Default)]
pub struct ArchiveStats {
    pub collections: usize,
    pub photos: usize,
    pub total_bytes: u64,
    /// Photos per collection year.
    pub photos_per_year: BTreeMap<i32, usize>,
    /// `(title, id, photos, bytes)`, largest first.
    pub largest: Vec<(String, Uuid, usize, u64)>,
    /// `(camera, photos)`, most used first.
    pub cameras: Vec<(String, usize)>,
    pub favorites: usize,
}

/// Computes statistics from the in-memory index.
pub fn compute(archive: &Archive) -> ArchiveStats {
    let mut s = ArchiveStats {
        collections: archive.collections.len(),
        ..Default::default()
    };
    let mut cameras: HashMap<String, usize> = HashMap::new();
    for c in &archive.collections {
        let d = &c.data;
        s.photos += d.photos.len();
        let bytes = d.total_bytes();
        s.total_bytes += bytes;
        *s.photos_per_year.entry(d.date.year()).or_default() += d.photos.len();
        s.largest
            .push((d.title.clone(), d.id, d.photos.len(), bytes));
        for p in &d.photos {
            if let Some(cam) = &p.camera {
                *cameras.entry(cam.clone()).or_default() += 1;
            }
            if p.favorite {
                s.favorites += 1;
            }
        }
    }
    s.largest.sort_by(|a, b| b.3.cmp(&a.3).then(b.2.cmp(&a.2)));
    s.largest.truncate(10);
    s.cameras = cameras.into_iter().collect();
    s.cameras
        .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    s.cameras.truncate(10);
    s
}

/// Collections from the same month/day in earlier years: `(id, years ago)`.
pub fn on_this_day(archive: &Archive, today: NaiveDate) -> Vec<(Uuid, i32)> {
    archive
        .collections
        .iter()
        .filter(|c| {
            c.data.date.month() == today.month()
                && c.data.date.day() == today.day()
                && c.data.date.year() < today.year()
        })
        .map(|c| (c.data.id, today.year() - c.data.date.year()))
        .collect()
}

/// A collection with GPS-tagged photos.
#[derive(Clone, Debug)]
pub struct MapEntry {
    pub collection_id: Uuid,
    pub title: String,
    pub date: NaiveDate,
    pub photos_with_gps: usize,
    /// Average position of the collection's photos.
    pub center: Gps,
}

/// Collections that have GPS data.
pub fn map_entries(archive: &Archive) -> Vec<MapEntry> {
    archive
        .collections
        .iter()
        .filter_map(|c| {
            let pts: Vec<Gps> = c.data.photos.iter().filter_map(|p| p.gps).collect();
            if pts.is_empty() {
                return None;
            }
            let n = pts.len() as f64;
            let center = Gps {
                lat: pts.iter().map(|g| g.lat).sum::<f64>() / n,
                lon: pts.iter().map(|g| g.lon).sum::<f64>() / n,
            };
            Some(MapEntry {
                collection_id: c.data.id,
                title: c.data.title.clone(),
                date: c.data.date,
                photos_with_gps: pts.len(),
                center,
            })
        })
        .collect()
}
