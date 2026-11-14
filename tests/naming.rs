use std::collections::HashSet;

use chrono::NaiveDate;
use photovault::archive::naming::*;

#[test]
fn removes_forbidden_characters() {
    assert_eq!(
        sanitize_title("Trip: Petra/Wadi Rum?"),
        "Trip Petra Wadi Rum"
    );
    assert_eq!(sanitize_title("a<b>c\"d|e*f\\g"), "a b c d e f g");
    assert_eq!(sanitize_title("tab\tand\nnewline"), "tab and newline");
    assert_eq!(sanitize_title("ctrl\u{0007}char"), "ctrlchar");
}

#[test]
fn reserved_windows_names() {
    assert_eq!(sanitize_title("CON"), "CON_");
    assert_eq!(sanitize_title("con"), "con_");
    assert_eq!(sanitize_title("LPT9"), "LPT9_");
    assert_eq!(sanitize_title("NUL.txt"), "NUL_.txt");
    assert_eq!(sanitize_title("Console"), "Console");
    assert_eq!(sanitize_file_name("aux.jpg"), "aux_.jpg");
}

#[test]
fn dots_and_spaces() {
    assert_eq!(sanitize_title("   ...   "), UNTITLED);
    assert_eq!(sanitize_title(""), UNTITLED);
    assert_eq!(sanitize_title("Summer..."), "Summer");
    assert_eq!(
        sanitize_title("  many    spaces   here  "),
        "many spaces here"
    );
    assert_eq!(sanitize_title(".hidden"), "hidden");
}

#[test]
fn preserves_unicode() {
    assert_eq!(sanitize_title("رحلة البتراء"), "رحلة البتراء");
    assert_eq!(sanitize_title("Beach 🏖️ day 🌅"), "Beach 🏖️ day 🌅");
    assert_eq!(sanitize_title("Café Zürich ñ"), "Café Zürich ñ");
    let date = NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
    assert_eq!(
        collection_folder_name(date, "رحلة البتراء"),
        "2026-11-14 - رحلة البتراء"
    );
}

#[test]
fn long_titles_are_cut_on_char_boundary() {
    let long = "ب".repeat(200);
    let s = sanitize_title(&long);
    assert_eq!(s.chars().count(), MAX_TITLE_CHARS);
    let emoji = "😀".repeat(200);
    assert_eq!(sanitize_title(&emoji).chars().count(), MAX_TITLE_CHARS);
    // A cut that lands on a space must not leave a trailing space.
    let spaced = format!("{} tail", "x".repeat(79));
    assert_eq!(sanitize_title(&spaced), "x".repeat(79));
}

#[test]
fn collision_suffixes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("2026-11-14 - Trip")).unwrap();
    let mut taken = HashSet::new();
    assert_eq!(
        unique_name(dir.path(), "2026-11-14 - Trip", &taken, false),
        "2026-11-14 - Trip (2)"
    );
    std::fs::create_dir(dir.path().join("2026-11-14 - Trip (2)")).unwrap();
    assert_eq!(
        unique_name(dir.path(), "2026-11-14 - Trip", &taken, false),
        "2026-11-14 - Trip (3)"
    );

    taken.insert("img_0413.jpg".to_owned());
    assert_eq!(
        unique_name(dir.path(), "IMG_0413.jpg", &taken, true),
        "IMG_0413 (2).jpg"
    );
    assert_eq!(
        unique_name(dir.path(), "IMG_0414.jpg", &taken, true),
        "IMG_0414.jpg"
    );
}

#[test]
fn folder_matching_and_parsing() {
    let d = NaiveDate::from_ymd_opt(2025, 8, 2).unwrap();
    assert!(folder_matches("2025-08-02 - Wadi Rum", d, "Wadi Rum"));
    assert!(folder_matches("2025-08-02 - Wadi Rum (2)", d, "Wadi Rum"));
    assert!(!folder_matches("2025-08-02 - Wadi Rum (x)", d, "Wadi Rum"));
    assert!(!folder_matches("2025-08-03 - Wadi Rum", d, "Wadi Rum"));
    assert_eq!(
        parse_folder_name("2025-08-02 - رحلة"),
        Some((d, "رحلة".to_owned()))
    );
    assert_eq!(parse_folder_name("2025-08-02"), Some((d, String::new())));
}
