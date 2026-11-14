mod common;

use std::io::Write;

use chrono::NaiveDate;
use photovault::archive::Archive;
use photovault::archive::import::DupDecision;
use photovault::backup::{self, BackupError, BackupOptions};
use photovault::progress::JobCtl;

fn build_archive(root: &std::path::Path, src: &std::path::Path) -> Archive {
    let mut archive = Archive::create(root, "Round Trip 🌍").unwrap();
    let specs = [
        ("رحلة البتراء", (2024, 4, 12)),
        ("Café & Croissants ☕", (2025, 8, 2)),
        ("Graduation Day", (2026, 11, 14)),
        ("Wadi Rum 🏜️", (2026, 1, 3)),
        ("Ünïcödé", (2023, 12, 31)),
    ];
    for (seed, (title, (y, m, d))) in (1..).zip(specs) {
        let id = archive
            .create_collection(title, NaiveDate::from_ymd_opt(y, m, d).unwrap())
            .unwrap();
        let files = common::make_images(&src.join(title), "صورة", 10, seed);
        common::import(&mut archive, id, files, DupDecision::ImportAnyway);
        let e = archive.get_mut(id).unwrap();
        e.data.notes = format!("{title}\n").repeat(100_000 / (title.len() + 1) + 1);
        e.data.tags = vec!["عائلة".into(), "trip".into()];
        archive.save_collection(id, false).unwrap();
    }
    assert_eq!(archive.photo_count(), 50);
    archive
}

fn make_backup(archive: &Archive, dest: &std::path::Path) -> backup::Manifest {
    let plan = backup::plan_backup(&archive.root, None).unwrap();
    backup::create_backup(
        &archive.root,
        &archive.meta,
        &plan,
        dest,
        &BackupOptions { verify: true },
        &JobCtl::new(),
    )
    .unwrap()
}

#[test]
fn round_trip_is_exact() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Archive");
    let archive = build_archive(&root, &tmp.path().join("src"));
    // Trash must not be included.
    std::fs::create_dir_all(root.join(".trash/x")).unwrap();
    std::fs::write(root.join(".trash/x/junk.jpg"), b"junk").unwrap();

    let zip_path = tmp.path().join("PhotoVault-Backup.zip");
    let manifest = make_backup(&archive, &zip_path);
    assert!(!tmp.path().join("PhotoVault-Backup.zip.partial").exists());
    assert_eq!(manifest.stats.collections, 5);
    assert_eq!(manifest.stats.photos, 50);
    assert!(
        manifest
            .files
            .iter()
            .all(|f| f.path.starts_with("archive/") && !f.path.contains(".trash"))
    );
    // README is present and the zip opens with a generic reader.
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&zip_path).unwrap()).unwrap();
    assert!(zip.by_name("README.txt").is_ok());
    assert!(zip.by_name("archive/photovault.json").is_ok());

    let before = common::snapshot_files(&root);
    let preview = backup::read_manifest(&zip_path).unwrap();
    assert_eq!(preview.archive_name, "Round Trip 🌍");

    std::fs::remove_dir_all(&root).unwrap();
    let dest = tmp.path().join("Restored");
    let report = backup::restore_backup(&zip_path, &dest, &JobCtl::new()).unwrap();
    assert_eq!(report.files, manifest.files.len());
    assert!(!dest.join(".photovault-restoring").exists());

    let after = common::snapshot_files(&dest);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    for (path, (bytes, mtime)) in &before {
        let (b2, m2) = &after[path];
        assert_eq!(bytes, b2, "content of {path}");
        assert_eq!(mtime, m2, "mtime of {path}");
    }
    let restored = Archive::open(&dest, None).unwrap();
    assert_eq!(restored.collections.len(), 5);
    assert_eq!(restored.photo_count(), 50);
    assert!(restored.last_scan.adopted_orphans.is_empty());
    for e in &archive.collections {
        assert_eq!(&restored.get(e.data.id).unwrap().data, &e.data);
    }
}

#[test]
fn corrupt_byte_is_detected_and_named() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Archive");
    let mut archive = Archive::create(&root, "Corrupt").unwrap();
    let id = archive
        .create_collection("Trip", NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())
        .unwrap();
    let files = common::make_images(&tmp.path().join("src"), "IMG", 3, 9);
    common::import(&mut archive, id, files, DupDecision::Skip);
    let zip_path = tmp.path().join("b.zip");
    make_backup(&archive, &zip_path);

    // Find the stored bytes of one photo inside the zip and flip one byte.
    let target_name = archive.get(id).unwrap().data.photos[1].file_name.clone();
    let target = std::fs::read(archive.photo_path(id, &target_name).unwrap()).unwrap();
    let mut bytes = std::fs::read(&zip_path).unwrap();
    let pos = bytes
        .windows(target.len())
        .position(|w| w == target.as_slice())
        .expect("stored entry");
    bytes[pos + target.len() / 2] ^= 0xFF;
    std::fs::File::create(&zip_path)
        .unwrap()
        .write_all(&bytes)
        .unwrap();

    let dest = tmp.path().join("out");
    let err = backup::restore_backup(&zip_path, &dest, &JobCtl::new()).unwrap_err();
    match &err {
        BackupError::VerificationFailed(files) => {
            assert_eq!(files.len(), 1);
            assert!(files[0].ends_with(&target_name), "{files:?}");
        }
        other => panic!("unexpected error: {other}"),
    }
    assert!(err.to_string().contains(&target_name));
    assert!(!dest.exists(), "failed restore leaves nothing behind");
    // Verification of the backup itself also fails.
    assert!(backup::verify_backup(&zip_path, &JobCtl::new()).is_err());
}

#[test]
fn zip_slip_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let zip_path = tmp.path().join("evil.zip");
    let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    let manifest = serde_json::json!({
        "format": "photovault-backup", "format_version": 1, "app_version": "0.1.0",
        "created_at": "2026-11-14T00:00:00Z", "source_os": "linux",
        "archive_id": uuid::Uuid::new_v4(), "archive_name": "evil",
        "stats": {"collections": 0, "photos": 0, "total_bytes": 4},
        "files": [{"path": "archive/../evil.txt", "size": 4, "sha256": "x", "mtime": 0}]
    });
    w.start_file("manifest.json", opts).unwrap();
    w.write_all(manifest.to_string().as_bytes()).unwrap();
    w.start_file("archive/../evil.txt", opts).unwrap();
    w.write_all(b"evil").unwrap();
    w.finish().unwrap();

    let dest = tmp.path().join("dest");
    let err = backup::restore_backup(&zip_path, &dest, &JobCtl::new()).unwrap_err();
    assert!(matches!(err, BackupError::UnsafePath(_)), "{err}");
    assert!(!tmp.path().join("evil.txt").exists());
    assert!(!dest.exists());

    for bad in ["/etc/passwd", "../x", "a/../../x", "C:/x", "a\\..\\x"] {
        assert!(backup::restore::check_entry_name(bad).is_err(), "{bad}");
    }
    assert!(backup::restore::check_entry_name("archive/2026/رحلة/صورة.jpg").is_ok());
}

#[test]
fn refuses_newer_format_and_non_empty_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let zip_path = tmp.path().join("new.zip");
    let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
    w.start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    w.write_all(br#"{"format":"photovault-backup","format_version":2}"#)
        .unwrap();
    w.finish().unwrap();
    assert!(matches!(
        backup::read_manifest(&zip_path),
        Err(BackupError::UnsupportedVersion(2))
    ));

    let root = tmp.path().join("Archive");
    let archive = Archive::create(&root, "x").unwrap();
    let zip_ok = tmp.path().join("ok.zip");
    make_backup(&archive, &zip_ok);
    let busy = tmp.path().join("busy");
    std::fs::create_dir_all(&busy).unwrap();
    std::fs::write(busy.join("file.txt"), b"x").unwrap();
    assert!(matches!(
        backup::restore_backup(&zip_ok, &busy, &JobCtl::new()),
        Err(BackupError::DestinationNotEmpty(_))
    ));
    let resolved = backup::resolve_destination(&busy);
    assert_eq!(resolved, busy.join("PhotoVault Archive (Restored)"));
    backup::restore_backup(&zip_ok, &resolved, &JobCtl::new()).unwrap();
}

#[test]
fn cancelled_backup_leaves_no_partial() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Archive");
    let archive = build_archive(&root, &tmp.path().join("src"));
    let plan = backup::plan_backup(&root, None).unwrap();
    let ctl = JobCtl::new();
    ctl.cancel();
    let dest = tmp.path().join("c.zip");
    let err = backup::create_backup(
        &root,
        &archive.meta,
        &plan,
        &dest,
        &BackupOptions::default(),
        &ctl,
    )
    .unwrap_err();
    assert!(matches!(err, BackupError::Cancelled));
    assert!(!dest.exists());
    assert!(!tmp.path().join("c.zip.partial").exists());
}

#[test]
fn incremental_backup_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Archive");
    let src = tmp.path().join("src");
    let mut archive = Archive::create(&root, "Inc").unwrap();
    let a = archive
        .create_collection("A", NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())
        .unwrap();
    common::import(
        &mut archive,
        a,
        common::make_images(&src.join("1"), "X", 4, 1),
        DupDecision::Skip,
    );
    let full_zip = tmp.path().join("full.zip");
    let full = make_backup(&archive, &full_zip);

    // Changes: new collection, edited notes, removed photo.
    let b = archive
        .create_collection("B", NaiveDate::from_ymd_opt(2026, 2, 2).unwrap())
        .unwrap();
    common::import(
        &mut archive,
        b,
        common::make_images(&src.join("2"), "Y", 2, 2),
        DupDecision::Skip,
    );
    archive.get_mut(a).unwrap().data.notes = "changed".into();
    let gone = archive.get(a).unwrap().data.photos[0].file_name.clone();
    archive
        .remove_photos(a, std::slice::from_ref(&gone))
        .unwrap();

    let plan = backup::plan_backup(&root, Some(&full)).unwrap();
    assert!(plan.included_files < plan.files.len());
    let inc_zip = tmp.path().join("inc.zip");
    let inc = backup::create_backup(
        &root,
        &archive.meta,
        &plan,
        &inc_zip,
        &BackupOptions::default(),
        &JobCtl::new(),
    )
    .unwrap();
    assert_eq!(inc.kind, backup::BackupKind::Incremental);
    assert!(inc.files.len() < inc.snapshot.len());

    let dest = tmp.path().join("restored");
    backup::restore_backup(&full_zip, &dest, &JobCtl::new()).unwrap();
    assert!(backup::restore_backup(&inc_zip, &tmp.path().join("nope"), &JobCtl::new()).is_err());
    backup::apply_incremental(&inc_zip, &dest, &JobCtl::new()).unwrap();

    let want = common::snapshot_files(&root);
    let got = common::snapshot_files(&dest);
    assert_eq!(
        want.keys().collect::<Vec<_>>(),
        got.keys().collect::<Vec<_>>()
    );
    for (k, (bytes, _)) in &want {
        assert_eq!(bytes, &got[k].0, "{k}");
    }
}
