//! Every `t!("…")` key used in the source must exist in the English table.

use std::collections::HashSet;

#[test]
fn all_keys_have_english_strings() {
    let keys: Vec<&str> = photovault::i18n::english_keys().collect();
    let set: HashSet<&str> = keys.iter().copied().collect();
    assert_eq!(set.len(), keys.len(), "duplicate keys in en.rs");

    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut missing = Vec::new();
    for entry in walkdir::WalkDir::new(&src)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "rs")
            || path.components().any(|c| c.as_os_str() == "i18n")
            || path.ends_with("i18n.rs")
        {
            continue;
        }
        let text = std::fs::read_to_string(path).unwrap();
        let mut rest = text.as_str();
        for marker in ["tr_logical(\"", "t!(\""] {
            let mut r2 = text.as_str();
            while let Some(pos) = r2.find(marker) {
                r2 = &r2[pos + marker.len()..];
                let key = &r2[..r2.find('"').unwrap()];
                if key.contains('.') && !set.contains(key) && marker.starts_with("tr_") {
                    missing.push(format!("{}: {key}", path.display()));
                }
            }
        }
        while let Some(pos) = rest.find("t!(") {
            let is_macro =
                pos == 0 || !rest[..pos].ends_with(|c: char| c.is_alphanumeric() || c == '_');
            rest = &rest[pos + 3..];
            if !is_macro {
                continue;
            }
            let trimmed = rest.trim_start();
            if let Some(after_quote) = trimmed.strip_prefix('"') {
                let key = &after_quote[..after_quote.find('"').unwrap()];
                if !set.contains(key) {
                    missing.push(format!("{}: {key}", path.display()));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "missing translations:\n{}",
        missing.join("\n")
    );
}

#[test]
fn placeholders_are_substituted() {
    assert_eq!(photovault::t!("sidebar.photos", n = 3), "3 photos");
    assert_eq!(photovault::t!("app.name"), "PhotoVault");
}

#[test]
fn arabic_table_matches_english() {
    use photovault::config::Language;
    let en: std::collections::HashMap<&str, &str> = photovault::i18n::strings(Language::En)
        .iter()
        .copied()
        .collect();
    let ar = photovault::i18n::strings(Language::Ar);
    let placeholders = |s: &str| {
        let mut v: Vec<String> = s
            .split('{')
            .skip(1)
            .filter_map(|p| p.split('}').next().map(String::from))
            .collect();
        v.sort();
        v
    };
    let mut missing: Vec<&str> = en
        .keys()
        .copied()
        .filter(|k| !ar.iter().any(|(a, _)| a == k))
        .collect();
    missing.sort();
    assert!(missing.is_empty(), "keys missing in Arabic: {missing:?}");
    for (k, v) in ar {
        let e = en
            .get(k)
            .unwrap_or_else(|| panic!("unknown Arabic key {k}"));
        assert_eq!(
            placeholders(e),
            placeholders(v),
            "placeholders differ for {k}"
        );
    }
}
