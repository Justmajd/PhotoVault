//! Bidirectional text for display (Unicode Bidi Algorithm, UAX #9).
//!
//! egui shapes text with HarfBuzz but has no bidi layout: it splits a line
//! into runs wherever the *font* changes, shapes each run (right-to-left for
//! Arabic script) and places the runs left to right. The UI registers the
//! Latin font first, so every maximal sequence of Arabic-script characters is
//! its own run that egui reverses, while digits, spaces and Latin text stay
//! left-to-right.
//!
//! [`visual_line`] therefore produces the line in final *visual* order and
//! then pre-reverses every Arabic-script sequence, so that egui's own
//! reversal restores it. The result renders correctly in any egui widget.
//! It is for display only: data on disk always stays in logical order.

use std::ops::Range;

use unicode_bidi::{Level, ParagraphBidiInfo};

/// `true` if `s` contains any right-to-left script character.
pub fn has_rtl(s: &str) -> bool {
    s.chars().any(is_rtl_script)
}

/// Characters drawn by the RTL (non-Latin) font: Hebrew, Arabic, Syriac,
/// Thaana, NKo, presentation forms, plus ZWNJ/ZWJ which belong to words.
pub fn is_rtl_script(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFC | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
}

fn joins_rtl_run(c: char) -> bool {
    is_rtl_script(c) || c == '\u{200C}' || c == '\u{200D}'
}

/// Mirrored glyph for characters displayed inside a right-to-left run.
fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        c => c,
    }
}

/// Base direction of a paragraph (its first strong character).
pub fn is_rtl_paragraph(paragraph: &str) -> bool {
    has_rtl(paragraph)
        && ParagraphBidiInfo::new(paragraph, None)
            .paragraph_level
            .is_rtl()
}

/// One displayed character: the char to draw and its byte index in the source.
pub type DisplayChar = (char, usize);

/// Display order for `range` of `paragraph` (a single line of text), with
/// `group` splitting Arabic-script sequences (e.g. at highlight boundaries,
/// because egui shapes each section separately).
fn display_chars(
    info: &ParagraphBidiInfo,
    range: Range<usize>,
    group: &dyn Fn(usize) -> u32,
) -> Vec<DisplayChar> {
    let text = info.text;
    let (levels, runs) = info.visual_runs(range);
    // 1. Full visual order (rule L2), mirroring inside RTL runs (rule L4).
    let mut out: Vec<DisplayChar> = Vec::with_capacity(text.len());
    for run in runs {
        let rtl = levels[run.start].is_rtl();
        let chars = text[run.clone()]
            .char_indices()
            .map(|(i, c)| (c, run.start + i));
        if rtl {
            out.extend(chars.rev().map(|(c, i)| (mirror(c), i)));
        } else {
            out.extend(chars);
        }
    }
    // 2. Pre-reverse every Arabic-script sequence (per group): egui reverses
    //    those runs when shaping, which restores the correct look.
    let mut i = 0;
    while i < out.len() {
        if joins_rtl_run(out[i].0) {
            let g = group(out[i].1);
            let mut j = i + 1;
            while j < out.len() && joins_rtl_run(out[j].0) && group(out[j].1) == g {
                j += 1;
            }
            out[i..j].reverse();
            i = j;
        } else {
            i += 1;
        }
    }
    // 3. Bidi control characters (isolates, marks) have done their job.
    out.retain(|(c, _)| !is_bidi_control(*c));
    out
}

/// Explicit bidi formatting characters (never drawn).
pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Wraps `s` in a first-strong isolate so it keeps its own direction when
/// embedded in a sentence (e.g. "93.1 KB" inside Arabic text).
pub fn isolate(s: &str) -> String {
    format!("\u{2068}{s}\u{2069}")
}

fn info_for(line: &str, base_rtl: Option<bool>) -> ParagraphBidiInfo<'_> {
    let level = base_rtl.map(|rtl| if rtl { Level::rtl() } else { Level::ltr() });
    ParagraphBidiInfo::new(line, level)
}

/// One line (no `\n`) converted for display.
pub fn visual_line(line: &str) -> String {
    if !has_rtl(line) {
        return line.chars().filter(|c| !is_bidi_control(*c)).collect();
    }
    let info = info_for(line, None);
    display_chars(&info, 0..line.len(), &|_| 0)
        .into_iter()
        .map(|(c, _)| c)
        .collect()
}

/// Text converted for display, line by line. A no-op for text without RTL characters.
pub fn visual_string(s: &str) -> String {
    if !has_rtl(s) {
        return s.chars().filter(|c| !is_bidi_control(*c)).collect();
    }
    s.split('\n')
        .map(visual_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A wrapped paragraph: `rows` are byte ranges of `paragraph` (as produced by
/// wrapping the logical text). Returns the paragraph direction and each row
/// ready for display (reordered with the paragraph's own levels, rule L1–L2).
pub fn visual_rows(paragraph: &str, rows: &[Range<usize>]) -> (bool, Vec<String>) {
    if !has_rtl(paragraph) {
        return (
            false,
            rows.iter()
                .map(|r| paragraph[r.clone()].to_owned())
                .collect(),
        );
    }
    let info = info_for(paragraph, None);
    let rtl = info.paragraph_level.is_rtl();
    let out = rows
        .iter()
        .map(|r| {
            if r.is_empty() {
                String::new()
            } else {
                display_chars(&info, r.clone(), &|_| 0)
                    .into_iter()
                    .map(|(c, _)| c)
                    .collect()
            }
        })
        .collect();
    (rtl, out)
}

/// A single line split into display segments `(text, highlighted)` where the
/// byte range `hl` of `line` is highlighted.
pub fn visual_highlight(line: &str, hl: (usize, usize)) -> Vec<(String, bool)> {
    let inside = |i: usize| i >= hl.0 && i < hl.1;
    let chars: Vec<DisplayChar> = if has_rtl(line) {
        let info = info_for(line, None);
        display_chars(&info, 0..line.len(), &|i| u32::from(inside(i)))
    } else {
        line.char_indices().map(|(i, c)| (c, i)).collect()
    };
    let mut out: Vec<(String, bool)> = Vec::new();
    for (c, i) in chars {
        let h = inside(i);
        match out.last_mut() {
            Some((s, last)) if *last == h => s.push(c),
            _ => out.push((c.to_string(), h)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What egui does to a display string: reverse each Arabic-script run.
    fn egui_shape(s: &str) -> String {
        let chars: Vec<char> = s.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            if joins_rtl_run(chars[i]) {
                let mut j = i;
                while j < chars.len() && joins_rtl_run(chars[j]) {
                    j += 1;
                }
                out.extend(chars[i..j].iter().rev());
                i = j;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out.into_iter().collect()
    }

    /// The expected left-to-right glyph order for a correctly displayed line.
    fn true_visual(s: &str) -> String {
        let info = ParagraphBidiInfo::new(s, None);
        let (levels, runs) = info.visual_runs(0..s.len());
        runs.into_iter()
            .map(|r| {
                if levels[r.start].is_rtl() {
                    s[r].chars().rev().map(mirror).collect::<String>()
                } else {
                    s[r].to_owned()
                }
            })
            .collect()
    }

    #[test]
    fn latin_is_untouched() {
        assert_eq!(visual_string("Hello (2025)"), "Hello (2025)");
    }

    #[test]
    fn rendering_matches_uba() {
        for s in [
            "رحلة البتراء",
            "عام 2025 كان جميلاً",
            "زرنا Petra في 12 نيسان",
            "Trip to البتراء in 2024",
            "٣ صور (جديدة)",
            "Backup: نسخة احتياطية 3 ملفات",
        ] {
            assert_eq!(egui_shape(&visual_line(s)), true_visual(s), "{s}");
        }
    }

    #[test]
    fn digits_keep_their_order() {
        let shown = egui_shape(&visual_line("عام 2025 كان"));
        assert!(shown.contains("2025"));
        // RTL paragraph: the first logical word is drawn rightmost.
        assert!(shown.ends_with(&"عام".chars().rev().collect::<String>()));
    }

    #[test]
    fn wrapped_rows_use_paragraph_direction() {
        let p = "Petra هي مدينة جميلة";
        let split = p.find(" مدينة").unwrap();
        let (rtl, rows) = visual_rows(p, &[0..split, split..p.len()]);
        assert!(!rtl);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn isolated_values_keep_their_order() {
        let s = format!("{} صورة · {}", isolate("6"), isolate("93.1 KB"));
        let shown = egui_shape(&visual_line(&s));
        assert!(shown.contains("93.1 KB"), "{shown}");
        assert!(!shown.contains('\u{2068}'));
        // Without isolation UBA puts the unit first, which reads wrong.
        let raw = egui_shape(&visual_line("6 صورة · 93.1 KB"));
        assert!(raw.contains("KB 93.1"), "{raw}");
    }

    #[test]
    fn hash_tag_sign_goes_to_reading_start() {
        assert_eq!(visual_line("#سفر"), "سفر#");
    }

    #[test]
    fn highlight_segments_cover_text() {
        let line = "زرنا البتراء أمس";
        let a = line.find("البتراء").unwrap();
        let segs = visual_highlight(line, (a, a + "البتراء".len()));
        assert!(segs.iter().any(|(_, h)| *h));
        let total: usize = segs.iter().map(|(s, _)| s.chars().count()).sum();
        assert_eq!(total, line.chars().count());
    }
}
