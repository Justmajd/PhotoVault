//! Case-insensitive, Unicode-aware search across collections.

use chrono::NaiveDate;
use uuid::Uuid;

use crate::archive::Archive;

/// Where a match was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchField {
    Title,
    Notes,
    Tag,
    Caption(String),
    FileName(String),
}

/// One search hit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub collection_id: Uuid,
    pub collection_title: String,
    pub date: NaiveDate,
    pub field: MatchField,
    /// Text around the match.
    pub snippet: String,
    /// Byte range of the match inside `snippet`.
    pub highlight: (usize, usize),
}

/// Optional filters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchFilters {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub only_favorites: bool,
    pub tag: Option<String>,
}

fn lower_chars(s: &str) -> Vec<char> {
    s.chars().flat_map(char::to_lowercase).collect()
}

/// Finds `needle` in `hay` ignoring case. Returns the byte range in `hay`.
pub fn find_ci(hay: &str, needle: &str) -> Option<(usize, usize)> {
    let needle = lower_chars(needle);
    if needle.is_empty() {
        return None;
    }
    let indices: Vec<(usize, char)> = hay.char_indices().collect();
    'outer: for (start_i, &(start_byte, _)) in indices.iter().enumerate() {
        let mut matched = 0;
        for &(byte, c) in &indices[start_i..] {
            for lc in c.to_lowercase() {
                if matched >= needle.len() || lc != needle[matched] {
                    continue 'outer;
                }
                matched += 1;
            }
            if matched == needle.len() {
                return Some((start_byte, byte + c.len_utf8()));
            }
        }
    }
    None
}

/// Builds a snippet of up to `context` chars around `range`.
pub fn snippet(text: &str, range: (usize, usize), context: usize) -> (String, (usize, usize)) {
    let before: Vec<(usize, char)> = text[..range.0].char_indices().collect();
    let start = if before.len() > context {
        before[before.len() - context].0
    } else {
        0
    };
    let after_end = text[range.1..]
        .char_indices()
        .nth(context)
        .map(|(i, _)| range.1 + i)
        .unwrap_or(text.len());
    let mut s = String::new();
    let prefix = if start > 0 { "…" } else { "" };
    s.push_str(prefix);
    let body = &text[start..after_end];
    // Flatten newlines so the snippet fits on one line (same byte length).
    s.push_str(&body.replace(['\n', '\r', '\t'], " "));
    let hl_start = prefix.len() + (range.0 - start);
    let hl_end = hl_start + (range.1 - range.0);
    if after_end < text.len() {
        s.push('…');
    }
    (s, (hl_start, hl_end))
}

/// Whether a collection passes the filters.
pub fn passes(c: &crate::archive::model::Collection, f: &SearchFilters) -> bool {
    if let Some(from) = f.from
        && c.date < from
    {
        return false;
    }
    if let Some(to) = f.to
        && c.date > to
    {
        return false;
    }
    if f.only_favorites && !c.favorite && !c.photos.iter().any(|p| p.favorite) {
        return false;
    }
    if let Some(tag) = &f.tag
        && !c.tags.iter().any(|t| t == tag)
    {
        return false;
    }
    true
}

/// Searches the archive. An empty query with filters lists matching collections by title.
pub fn search(
    archive: &Archive,
    query: &str,
    filters: &SearchFilters,
    limit: usize,
) -> Vec<SearchHit> {
    let query = query.trim();
    let mut hits = Vec::new();
    for entry in &archive.collections {
        let c = &entry.data;
        if !passes(c, filters) {
            continue;
        }
        let mut push = |field: MatchField, text: &str, range: (usize, usize)| {
            let (snip, hl) = snippet(text, range, 40);
            hits.push(SearchHit {
                collection_id: c.id,
                collection_title: c.title.clone(),
                date: c.date,
                field,
                snippet: snip,
                highlight: hl,
            });
        };
        if query.is_empty() {
            push(MatchField::Title, &c.title, (0, 0));
            continue;
        }
        if let Some(r) = find_ci(&c.title, query) {
            push(MatchField::Title, &c.title, r);
        }
        if let Some(r) = find_ci(&c.notes, query) {
            push(MatchField::Notes, &c.notes, r);
        }
        for t in &c.tags {
            if let Some(r) = find_ci(t, query) {
                push(MatchField::Tag, t, r);
            }
        }
        for p in &c.photos {
            if let Some(r) = find_ci(&p.caption, query) {
                push(MatchField::Caption(p.file_name.clone()), &p.caption, r);
            }
            if let Some(r) = find_ci(&p.file_name, query) {
                push(MatchField::FileName(p.file_name.clone()), &p.file_name, r);
            } else if p.original_name != p.file_name
                && let Some(r) = find_ci(&p.original_name, query)
            {
                push(
                    MatchField::FileName(p.file_name.clone()),
                    &p.original_name,
                    r,
                );
            }
        }
        if hits.len() >= limit {
            break;
        }
    }
    hits.truncate(limit);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_ci() {
        assert_eq!(find_ci("Hello WORLD", "world"), Some((6, 11)));
        assert_eq!(find_ci("Ünïcödé Straße", "STRASSE"), None);
        assert_eq!(find_ci("Ünïcödé", "üNÏ"), Some((0, 5)));
        let s = "رحلة البتراء الجميلة";
        let (a, b) = find_ci(s, "البتراء").unwrap();
        assert_eq!(&s[a..b], "البتراء");
    }

    #[test]
    fn snippet_bounds() {
        let text = "a".repeat(100) + "needle" + &"b".repeat(100);
        let r = find_ci(&text, "NEEDLE").unwrap();
        let (s, (x, y)) = snippet(&text, r, 10);
        assert_eq!(&s[x..y], "needle");
    }
}
