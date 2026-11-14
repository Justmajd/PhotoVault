//! Folder and file naming: sanitization and collision-free names.
//!
//! Names must be valid on Windows, macOS and Linux at the same time, while
//! preserving Unicode (Arabic, emoji, accents) exactly.

use std::collections::HashSet;
use std::path::Path;

use chrono::NaiveDate;

/// Maximum length (in chars) of a sanitized collection title.
pub const MAX_TITLE_CHARS: usize = 80;
/// Maximum length (in chars) of a sanitized file stem.
pub const MAX_FILE_STEM_CHARS: usize = 120;
/// Title used when nothing usable is left after sanitization.
pub const UNTITLED: &str = "Untitled";

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_forbidden(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control()
}

/// Returns `true` if `name` (or its stem before the first dot) is a reserved
/// Windows device name.
pub fn is_reserved_windows_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end();
    RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem))
}

/// Core cleanup: drop forbidden chars, collapse whitespace, limit length,
/// trim trailing dots/spaces. Returns an empty string if nothing is left.
fn clean(input: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_space = false;
    for c in input.chars() {
        if c.is_control() && !c.is_whitespace() {
            continue;
        }
        // Forbidden characters are removed; they act as word separators so
        // that "Petra/Wadi Rum" becomes "Petra Wadi Rum", not "PetraWadi Rum".
        if is_forbidden(c) || c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    let mut truncated: String = out.chars().take(max_chars).collect();
    // Windows does not allow trailing dots or spaces; also strip leading ones
    // so that we never create hidden dot folders.
    loop {
        let trimmed = truncated
            .trim_end_matches(['.', ' '])
            .trim_start_matches(['.', ' ']);
        if trimmed.len() == truncated.len() {
            break;
        }
        truncated = trimmed.to_owned();
    }
    truncated
}

/// Sanitizes a collection title for use in a folder name.
pub fn sanitize_title(title: &str) -> String {
    let mut s = clean(title, MAX_TITLE_CHARS);
    if s.is_empty() {
        s = UNTITLED.to_owned();
    }
    fix_reserved(&mut s);
    s
}

/// Appends `_` to a reserved device-name stem (`NUL.txt` → `NUL_.txt`).
fn fix_reserved(s: &mut String) {
    if is_reserved_windows_name(s) {
        let stem_len = s.split('.').next().unwrap_or("").trim_end().len();
        s.insert(stem_len, '_');
    }
}

/// The canonical (collision-free before suffixing) folder name.
pub fn collection_folder_name(date: NaiveDate, title: &str) -> String {
    format!("{} - {}", date.format("%Y-%m-%d"), sanitize_title(title))
}

/// The year folder name for a date.
pub fn year_folder_name(date: NaiveDate) -> String {
    date.format("%Y").to_string()
}

/// Appends ` (n)` to `base` (before the extension for files).
pub fn with_suffix(base: &str, n: u32, is_file: bool) -> String {
    if is_file && let Some((stem, ext)) = split_ext(base) {
        return format!("{stem} ({n}).{ext}");
    }
    format!("{base} ({n})")
}

/// Splits `name` into `(stem, extension)` if it has a non-empty extension.
pub fn split_ext(name: &str) -> Option<(&str, &str)> {
    let idx = name.rfind('.')?;
    if idx == 0 || idx == name.len() - 1 {
        return None;
    }
    Some((&name[..idx], &name[idx + 1..]))
}

/// Returns `base` or `base (2)`, `base (3)`, … so that it is not in `taken`
/// (compared case-insensitively) and does not exist inside `dir`.
pub fn unique_name(dir: &Path, base: &str, taken: &HashSet<String>, is_file: bool) -> String {
    let is_free = |candidate: &str| {
        !taken.contains(&candidate.to_lowercase()) && !dir.join(candidate).exists()
    };
    if is_free(base) {
        return base.to_owned();
    }
    let mut n = 2;
    loop {
        let candidate = with_suffix(base, n, is_file);
        if is_free(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Sanitizes an imported file name, keeping (and lower-casing nothing of) its extension.
pub fn sanitize_file_name(name: &str) -> String {
    let (stem, ext) = match split_ext(name) {
        Some((s, e)) => (s.to_owned(), Some(clean(e, 16))),
        None => (name.to_owned(), None),
    };
    let mut stem = clean(&stem, MAX_FILE_STEM_CHARS);
    if stem.is_empty() {
        stem = "photo".to_owned();
    }
    fix_reserved(&mut stem);
    match ext {
        Some(e) if !e.is_empty() => format!("{stem}.{e}"),
        _ => stem,
    }
}

/// Returns `true` if the folder `name` is the canonical name for `date`/`title`,
/// possibly with a collision suffix ` (n)`.
pub fn folder_matches(name: &str, date: NaiveDate, title: &str) -> bool {
    let canonical = collection_folder_name(date, title);
    if name == canonical {
        return true;
    }
    if let Some(rest) = name.strip_prefix(&canonical)
        && let Some(num) = rest.strip_prefix(" (").and_then(|r| r.strip_suffix(')'))
    {
        return num.parse::<u32>().is_ok_and(|n| n >= 2);
    }
    false
}

/// Parses `YYYY-MM-DD - Title` (or `YYYY-MM-DD Title`, `YYYY-MM-DD`) folder names.
pub fn parse_folder_name(name: &str) -> Option<(NaiveDate, String)> {
    let date_part: String = name.chars().take(10).collect();
    let date = NaiveDate::parse_from_str(&date_part, "%Y-%m-%d").ok()?;
    let rest = name[date_part.len()..]
        .trim_start_matches([' ', '-', '_', '–', '—'])
        .trim();
    // Strip a trailing collision suffix like " (2)".
    let rest = match rest.rfind(" (") {
        Some(idx)
            if rest.ends_with(')') && rest[idx + 2..rest.len() - 1].parse::<u32>().is_ok() =>
        {
            &rest[..idx]
        }
        _ => rest,
    };
    Some((date, rest.to_owned()))
}

/// Backup file name for a timestamp: `PhotoVault-Backup-YYYY-MM-DD_HHMM.zip`.
pub fn backup_file_name(at: chrono::DateTime<chrono::Local>) -> String {
    format!("PhotoVault-Backup-{}.zip", at.format("%Y-%m-%d_%H%M"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_sanitize() {
        assert_eq!(
            sanitize_title("Trip: Petra/Wadi Rum?"),
            "Trip Petra Wadi Rum"
        );
        assert_eq!(sanitize_title("CON"), "CON_");
        assert_eq!(sanitize_title("   ...   "), UNTITLED);
    }

    #[test]
    fn parse_names() {
        let (d, t) = parse_folder_name("2025-08-02 - Wadi Rum Camping (3)").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2025, 8, 2).unwrap());
        assert_eq!(t, "Wadi Rum Camping");
        assert!(parse_folder_name("Holiday").is_none());
    }
}
