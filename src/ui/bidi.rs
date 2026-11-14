//! Bidi-aware text helpers for egui (see `photovault::bidi`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};
use photovault::bidi::{
    has_rtl, is_rtl_paragraph, visual_highlight, visual_line, visual_rows, visual_string,
};

/// Converts user text for display (no-op for left-to-right text).
pub fn vis(s: &str) -> String {
    if has_rtl(s) {
        photovault::i18n::display(s)
    } else {
        visual_string(s)
    }
}

/// Joins already-displayable pieces in reading order (reversed in the RTL UI).
pub fn join(parts: &[&str], sep: &str) -> String {
    if photovault::i18n::is_rtl() {
        parts.iter().rev().copied().collect::<Vec<_>>().join(sep)
    } else {
        parts.join(sep)
    }
}

/// A search snippet with a highlighted byte range, in display order.
pub fn highlight_job(
    snippet: &str,
    hl: (usize, usize),
    normal: TextFormat,
    strong: TextFormat,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (text, h) in visual_highlight(snippet, hl) {
        job.append(&text, 0.0, if h { strong.clone() } else { normal.clone() });
    }
    job
}

/// Rows of a wrapped block: `(right_to_left, display text)`.
type Rows = Arc<Vec<(bool, String)>>;

thread_local! {
    static WRAP_CACHE: RefCell<HashMap<(u64, u32, u32), Rows>> = RefCell::new(HashMap::new());
}

fn measure(ui: &egui::Ui, s: &str, font: &FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(visual_line(s), font.clone(), Color32::WHITE)
        .size()
        .x
}

/// Wraps `text` to `width` word by word in logical order, then reorders each
/// row for display with its paragraph's bidi levels.
fn wrap(ui: &egui::Ui, text: &str, font: &FontId, width: f32, rtl_ui: bool) -> Rows {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    font.size.to_bits().hash(&mut h);
    rtl_ui.hash(&mut h);
    let key = (
        h.finish(),
        width as u32,
        ui.ctx().pixels_per_point().to_bits(),
    );
    if let Some(rows) = WRAP_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return rows;
    }
    let mut out = Vec::new();
    for line in text.split('\n') {
        if line.trim().is_empty() {
            out.push((rtl_ui, String::new()));
            continue;
        }
        let mut ranges = Vec::new();
        let (mut start, mut cur) = (0usize, 0.0f32);
        let mut pos = 0usize;
        for token in line.split_inclusive(' ') {
            let w_word = measure(ui, token.trim_end(), font);
            let w_full = if token.ends_with(' ') {
                measure(ui, token, font).max(w_word)
            } else {
                w_word
            };
            if cur > 0.0 && cur + w_word > width {
                ranges.push(start..line[..pos].trim_end().len().max(start));
                start = pos;
                cur = 0.0;
            }
            cur += w_full;
            pos += token.len();
        }
        ranges.push(start..line.trim_end().len().max(start));
        let rtl = if has_rtl(line) {
            is_rtl_paragraph(line)
        } else {
            false
        };
        let (_, rows) = visual_rows(line, &ranges);
        out.extend(rows.into_iter().map(|r| (rtl, r)));
    }
    let rows: Rows = Arc::new(out);
    WRAP_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 256 {
            c.clear();
        }
        c.insert(key, rows.clone());
    });
    rows
}

/// A wrapped, bidi-correct text block. Right-to-left paragraphs are
/// right-aligned. Returns a clickable response covering the block.
pub fn paragraphs(
    ui: &mut egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    rtl_ui: bool,
) -> egui::Response {
    paragraphs_aligned(ui, text, font, color, rtl_ui, false)
}

/// Like [`paragraphs`], optionally centering every row.
pub fn paragraphs_aligned(
    ui: &mut egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    rtl_ui: bool,
    center: bool,
) -> egui::Response {
    let width = ui.available_width().max(50.0);
    let rows = wrap(ui, text, &font, width, rtl_ui);
    let row_h = ui.fonts_mut(|f| f.row_height(&font));
    let height = row_h * rows.len().max(1) as f32;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let clip = ui.clip_rect();
        for (i, (rtl, row)) in rows.iter().enumerate() {
            let y = rect.top() + i as f32 * row_h;
            if y + row_h < clip.top() || y > clip.bottom() {
                continue;
            }
            let g = ui
                .painter()
                .layout_no_wrap(row.clone(), font.clone(), color);
            let x = if center {
                rect.center().x - g.size().x / 2.0
            } else if *rtl {
                rect.right() - g.size().x
            } else {
                rect.left()
            };
            ui.painter().galley(egui::pos2(x, y), g, color);
        }
    }
    resp
}

/// Wrapped rows for painting by hand (e.g. inside custom-drawn cards).
pub fn rows(ui: &egui::Ui, logical: &str, font: &FontId, width: f32) -> Vec<(bool, String)> {
    wrap(ui, logical, font, width, photovault::i18n::is_rtl())
        .as_ref()
        .clone()
}

/// A wrapping label for display text from `t!()` (or plain LTR text). Text
/// containing RTL script is wrapped from its logical original so lines break
/// in reading order; LTR text uses a normal egui label.
pub fn para(ui: &mut egui::Ui, display: &str, size: f32, color: Option<Color32>) -> egui::Response {
    let color = color.unwrap_or_else(|| ui.visuals().text_color());
    if !has_rtl(display) {
        return ui
            .add(egui::Label::new(egui::RichText::new(display).size(size).color(color)).wrap());
    }
    let logical = photovault::i18n::logical_of(display).unwrap_or_else(|| display.to_owned());
    paragraphs(
        ui,
        &logical,
        FontId::proportional(size),
        color,
        photovault::i18n::is_rtl(),
    )
}
