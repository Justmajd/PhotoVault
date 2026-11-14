mod common;

use chrono::NaiveDate;
use photovault::archive::import::DupDecision;
use photovault::archive::model::{COLLECTION_FILE, NOTES_FILE};
use photovault::archive::{Archive, FolderKind, integrity};
use photovault::progress::JobCtl;

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[test]
fn create_import_edit_reload() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Archive");
    let src = tmp.path().join("src");
    let mut archive = Archive::create(&root, "Test Archive").unwrap();

    let a = archive
        .create_collection("Wadi Rum Camping", date(2025, 8, 2))
        .unwrap();
    let b = archive
        .create_collection("رحلة البتراء", date(2026, 3, 1))
        .unwrap();
    let c = archive
        .create_collection("Graduation Day", date(2026, 11, 14))
        .unwrap();
    assert!(
        root.join("2025/2025-08-02 - Wadi Rum Camping/collection.json")
            .is_file()
    );
    assert!(
        root.join("2026/2026-03-01 - رحلة البتراء/NOTES.md")
            .is_file()
    );

    let files_a = common::make_images(&src.join("a"), "IMG", 4, 1);
    let files_b = common::make_images(&src.join("b"), "IMG", 3, 2);
    let r = common::import(&mut archive, a, files_a, DupDecision::Skip);
    assert_eq!(r.imported.len(), 4);
    common::import(&mut archive, b, files_b, DupDecision::Skip);
    assert_eq!(archive.photo_count(), 7);

    // Edit title → folder renamed; edit date → folder moves year.
    archive.get_mut(c).unwrap().data.title = "Graduation: Day One".into();
    let out = archive.save_collection(c, true).unwrap();
    assert!(out.renamed);
    assert!(root.join("2026/2026-11-14 - Graduation Day One").is_dir());
    assert!(!root.join("2026/2026-11-14 - Graduation Day").exists());

    archive.get_mut(a).unwrap().data.date = date(2024, 12, 31);
    archive.save_collection(a, true).unwrap();
    assert!(root.join("2024/2024-12-31 - Wadi Rum Camping").is_dir());
    assert!(!root.join("2025").exists(), "empty year folder is removed");

    // Notes and tags.
    {
        let e = archive.get_mut(b).unwrap();
        e.data.notes = "ملاحظات طويلة\nSecond line with emoji 🎓".into();
        e.data.tags = vec!["family".into(), "university".into()];
        e.data.photos[0].caption = "First photo".into();
    }
    archive.save_collection(b, false).unwrap();
    let notes_md =
        std::fs::read_to_string(root.join("2026/2026-03-01 - رحلة البتراء").join(NOTES_FILE))
            .unwrap();
    assert!(notes_md.contains("# رحلة البتراء"));
    assert!(notes_md.contains("Tags: family, university"));
    assert!(notes_md.contains("— First photo"));

    // Reload from disk and compare.
    let reloaded = Archive::open(&root, None).unwrap();
    assert_eq!(reloaded.meta, archive.meta);
    assert_eq!(reloaded.collections.len(), 3);
    for entry in &archive.collections {
        let other = reloaded.get(entry.data.id).unwrap();
        assert_eq!(other.data, entry.data);
        assert_eq!(other.rel_dir, entry.rel_dir);
    }
    assert!(reloaded.last_scan.adopted_orphans.is_empty());
}

#[test]
fn duplicates_orphans_and_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Archive");
    let src = tmp.path().join("src");
    let mut archive = Archive::create(&root, "Dup Test").unwrap();
    let a = archive.create_collection("A", date(2026, 1, 1)).unwrap();
    let b = archive.create_collection("B", date(2026, 1, 2)).unwrap();
    let files = common::make_images(&src, "P", 3, 7);

    let r1 = common::import(&mut archive, a, files.clone(), DupDecision::Skip);
    assert_eq!(r1.imported.len(), 3);
    // Same files again into the same collection → skipped silently.
    let r2 = common::import(&mut archive, a, files.clone(), DupDecision::ImportAnyway);
    assert_eq!(r2.imported.len(), 0);
    assert_eq!(r2.duplicates_skipped.len(), 3);
    // Into another collection: the answer decides.
    let r3 = common::import(&mut archive, b, files[..1].to_vec(), DupDecision::Skip);
    assert_eq!(r3.imported.len(), 0);
    let r4 = common::import(
        &mut archive,
        b,
        files[..1].to_vec(),
        DupDecision::ShowExisting,
    );
    assert_eq!(r4.show_existing.as_ref().unwrap().collection_id, a);
    let r5 = common::import(
        &mut archive,
        b,
        files[..1].to_vec(),
        DupDecision::ImportAnyway,
    );
    assert_eq!(r5.imported.len(), 1);
    // No temp files are left behind.
    let dir_a = archive.collection_dir(a).unwrap();
    assert!(
        std::fs::read_dir(&dir_a).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".pvtmp"))
    );

    // Orphan: drop a new image into the folder by hand.
    common::make_image(&dir_a.join("orphan.png"), 999);
    // Missing: delete a listed file.
    let first = archive.get(a).unwrap().data.photos[0].file_name.clone();
    std::fs::remove_file(dir_a.join(&first)).unwrap();

    let reloaded = Archive::open(&root, None).unwrap();
    let ea = reloaded.get(a).unwrap();
    assert_eq!(
        reloaded.last_scan.adopted_orphans,
        vec![("A".to_owned(), "orphan.png".to_owned())]
    );
    assert!(ea.data.photo_index("orphan.png").is_some());
    assert!(ea.missing.contains(&first));
    assert_eq!(reloaded.last_scan.missing, 1);
    // The adoption was persisted.
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir_a.join(COLLECTION_FILE)).unwrap()).unwrap();
    assert!(
        json["photos"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["file_name"] == "orphan.png")
    );

    // Integrity check finds the missing file and a modified one.
    let second = ea.data.photos[1].file_name.clone();
    std::fs::write(dir_a.join(&second), b"tampered").unwrap();
    let mut reloaded = reloaded;
    let report = integrity::check(&integrity::snapshot(&reloaded), &JobCtl::new());
    assert_eq!(report.missing.len(), 1);
    assert_eq!(report.modified.len(), 1);
    assert_eq!(report.modified[0].file_name, second);
    reloaded.fix_drop_missing(&report.missing).unwrap();
    reloaded.fix_update_hashes(&report.modified).unwrap();
    let again = integrity::check(&integrity::snapshot(&reloaded), &JobCtl::new());
    assert!(again.is_clean(), "{again:?}");
}

#[test]
fn leftover_temp_files_are_ignored_and_cleaned() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Archive");
    let mut archive = Archive::create(&root, "Crash Test").unwrap();
    let a = archive
        .create_collection("Crash", date(2026, 5, 5))
        .unwrap();
    let dir = archive.collection_dir(a).unwrap();
    // Simulate a crash in the middle of an atomic save and of an import copy.
    std::fs::write(
        dir.join(".collection.json.deadbeef.pvtmp"),
        b"{ half written",
    )
    .unwrap();
    std::fs::write(dir.join(".IMG_1.jpg.cafebabe.pvtmp"), b"partial jpeg").unwrap();
    std::fs::write(root.join(".photovault.json.12345678.pvtmp"), b"{").unwrap();

    let reloaded = Archive::open(&root, None).unwrap();
    assert_eq!(reloaded.last_scan.cleaned_temp, 3);
    assert_eq!(reloaded.get(a).unwrap().data, archive.get(a).unwrap().data);
    assert!(
        reloaded.get(a).unwrap().data.photos.is_empty(),
        "temp image was not adopted"
    );
    assert!(
        std::fs::read_dir(&dir).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".pvtmp"))
    );
}

#[test]
fn trash_and_undo() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Archive");
    let mut archive = Archive::create(&root, "Trash").unwrap();
    let a = archive.create_collection("A", date(2026, 2, 2)).unwrap();
    let b = archive.create_collection("B", date(2026, 2, 3)).unwrap();
    common::import(
        &mut archive,
        a,
        common::make_images(&tmp.path().join("s"), "X", 4, 3),
        DupDecision::Skip,
    );
    let before = archive.get(a).unwrap().data.clone();
    let names: Vec<String> = before.photos[1..3]
        .iter()
        .map(|p| p.file_name.clone())
        .collect();

    let batch = archive.remove_photos(a, &names).unwrap();
    assert_eq!(archive.get(a).unwrap().data.photos.len(), 2);
    assert!(archive.trash_size() > 0);
    archive.undo_trash(batch).unwrap();
    let after = archive.get(a).unwrap().data.clone();
    assert_eq!(after.photos, before.photos);

    // Move a photo to another collection.
    let moved = archive.move_photos(a, b, &names[..1]).unwrap();
    assert_eq!(moved, 1);
    assert!(archive.collection_dir(b).unwrap().join(&names[0]).is_file());

    let batch = archive.delete_collection(a).unwrap();
    assert!(archive.get(a).is_none());
    archive.undo_trash(batch).unwrap();
    assert_eq!(archive.get(a).unwrap().data.photos.len(), 3);
    let reloaded = Archive::open(&root, None).unwrap();
    assert_eq!(reloaded.collections.len(), 2);
    assert!(reloaded.last_scan.adopted_orphans.is_empty());
}

#[test]
fn adopt_folder_without_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Loose");
    common::make_images(&root.join("2024/2024-06-01 - Beach"), "B", 2, 5);
    assert_eq!(
        Archive::inspect_folder(&root),
        FolderKind::Adoptable { folders: 1 }
    );
    let archive = Archive::adopt_existing(&root, "Loose", None).unwrap();
    assert_eq!(archive.collections.len(), 1);
    let c = &archive.collections[0].data;
    assert_eq!(c.title, "Beach");
    assert_eq!(c.date, date(2024, 6, 1));
    assert_eq!(c.photos.len(), 2);
    assert_eq!(Archive::inspect_folder(&root), FolderKind::Archive);
}
