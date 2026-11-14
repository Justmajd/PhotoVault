//! First-run introduction: three short animated slides.

use std::time::Instant;

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Vec2, pos2, vec2};
use photovault::t;

use crate::anim::{ease_out, ease_out_back};
use crate::app::PhotoVaultApp;
use crate::theme;

const STEPS: usize = 3;

/// State of the introduction overlay.
pub struct IntroState {
    pub step: usize,
    /// Previous step (for the slide transition) and when the step changed.
    prev: Option<usize>,
    changed_at: Instant,
    opened_at: Instant,
    closing: Option<Instant>,
}

impl IntroState {
    pub fn new() -> Self {
        Self {
            step: 0,
            prev: None,
            changed_at: Instant::now(),
            opened_at: Instant::now(),
            closing: None,
        }
    }

    fn go(&mut self, step: usize) {
        self.prev = Some(self.step);
        self.step = step;
        self.changed_at = Instant::now();
    }
}

impl Default for IntroState {
    fn default() -> Self {
        Self::new()
    }
}

/// Draws the overlay. Returns nothing; closes itself through `app.intro`.
pub fn show(app: &mut PhotoVaultApp, ctx: &egui::Context) {
    let accent = app.accent();
    let anim = app.config.animations;
    let Some(state) = app.intro.as_mut() else {
        return;
    };
    let p = |start: Instant, secs: f32| crate::anim::progress(ctx, start, secs, anim);

    let open_t = ease_out(p(state.opened_at, 0.35));
    let close_t = state.closing.map(|c| ease_out(p(c, 0.25))).unwrap_or(0.0);
    if close_t >= 1.0 {
        app.intro = None;
        app.config.intro_seen = true;
        app.config_dirty = true;
        return;
    }
    let visible = open_t * (1.0 - close_t);
    let screen = ctx.content_rect();
    let mut finish = false;

    egui::Area::new(egui::Id::unique("intro_overlay"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.set_min_size(screen.size());
            // Dim backdrop that swallows clicks.
            let bg = ui.interact(
                screen,
                egui::Id::unique("intro_backdrop"),
                egui::Sense::click(),
            );
            let _ = bg;
            ui.painter().rect_filled(
                screen,
                CornerRadius::ZERO,
                Color32::from_black_alpha((170.0 * visible) as u8),
            );

            let card_size = vec2(560.0, 470.0).min(screen.size() - vec2(32.0, 32.0));
            let center = screen.center() + vec2(0.0, (1.0 - visible) * 24.0);
            let card = Rect::from_center_size(center, card_size * (0.96 + 0.04 * visible));
            let painter = ui.painter().clone();
            let mut painter = painter;
            painter.set_opacity(visible);
            painter.add(
                egui::epaint::Shadow {
                    offset: [0, 18],
                    blur: 48,
                    spread: 0,
                    color: Color32::from_black_alpha(90),
                }
                .as_shape(card, CornerRadius::same(22)),
            );
            painter.rect_filled(card, CornerRadius::same(22), ui.visuals().window_fill);

            // Slide transition: the new slide comes in from the reading-forward side.
            let slide_t = ease_out(p(state.changed_at, 0.38));
            let forward = state.prev.is_none_or(|prev| state.step > prev);
            let dir = if crate::ui::rtl() { -1.0 } else { 1.0 } * if forward { 1.0 } else { -1.0 };
            let content = card.shrink2(vec2(36.0, 30.0));
            let clip = painter.with_clip_rect(card.shrink(2.0));
            if let Some(prev) = state.prev
                && slide_t < 1.0
            {
                let off = -dir * slide_t * 80.0;
                let mut out = clip.clone();
                out.set_opacity(visible * (1.0 - slide_t));
                draw_slide(
                    &out,
                    ui,
                    prev,
                    content.translate(vec2(off, 0.0)),
                    accent,
                    1.0,
                    state.changed_at,
                );
            }
            let off = dir * (1.0 - slide_t) * 80.0;
            let mut cur = clip.clone();
            cur.set_opacity(visible * slide_t.max(if state.prev.is_none() { 1.0 } else { 0.0 }));
            let illus_t = ease_out(p(state.changed_at, 0.9));
            draw_slide(
                &cur,
                ui,
                state.step,
                content.translate(vec2(off, 0.0)),
                accent,
                illus_t,
                state.changed_at,
            );

            // Step dots (the active one stretches).
            let dots_y = card.bottom() - 74.0;
            let mut x = card.center().x - 30.0;
            for i in 0..STEPS {
                let i_vis = if crate::ui::rtl() { STEPS - 1 - i } else { i };
                let active = ui.ctx().animate_bool_with_time(
                    egui::Id::unique(("intro_dot", i_vis)),
                    state.step == i_vis,
                    if anim { 0.25 } else { 0.0 },
                );
                let w = 8.0 + active * 16.0;
                let r = Rect::from_min_size(pos2(x, dots_y), vec2(w, 8.0));
                painter.rect_filled(
                    r,
                    CornerRadius::same(4),
                    crate::anim::lerp_color(ui.visuals().widgets.inactive.bg_fill, accent, active),
                );
                x += w + 8.0;
            }

            // Buttons.
            let btn_area = Rect::from_min_max(
                pos2(card.left() + 28.0, card.bottom() - 54.0),
                pos2(card.right() - 28.0, card.bottom() - 18.0),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(btn_area)
                    .layout(crate::ui::row_layout()),
            );
            child.set_opacity(visible);
            if child.button(t!("intro.skip")).clicked() {
                finish = true;
            }
            child.with_layout(crate::ui::end_layout(), |ui| {
                let last = state.step + 1 == STEPS;
                let label = if last {
                    t!("intro.start")
                } else {
                    t!("intro.next")
                };
                if theme::primary_button(ui, accent, label).clicked() {
                    if last {
                        finish = true;
                    } else {
                        state.go(state.step + 1);
                    }
                }
                if state.step > 0
                    && ui
                        .button(if crate::ui::rtl() { "→" } else { "←" })
                        .clicked()
                {
                    state.go(state.step - 1);
                }
            });
            let (next, back, esc) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::Backspace),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if esc {
                finish = true;
            } else if next {
                if state.step + 1 == STEPS {
                    finish = true;
                } else {
                    state.go(state.step + 1);
                }
            } else if back && state.step > 0 {
                state.go(state.step - 1);
            }
        });
    if finish && state.closing.is_none() {
        state.closing = Some(Instant::now());
        if !anim {
            app.intro = None;
            app.config.intro_seen = true;
            app.config_dirty = true;
        }
    }
}

fn draw_slide(
    p: &egui::Painter,
    ui: &egui::Ui,
    step: usize,
    area: Rect,
    accent: Color32,
    t: f32,
    start: Instant,
) {
    let illus = Rect::from_center_size(
        pos2(area.center().x, area.top() + 105.0),
        vec2(220.0, 170.0),
    );
    match step {
        0 => draw_folder(p, ui, illus, accent, t),
        1 => draw_notes(p, ui, illus, accent, t),
        _ => draw_backup(p, ui, illus, accent, t, start),
    }
    let (title, body) = match step {
        0 => (t!("intro.1.title"), t!("intro.1.body")),
        1 => (t!("intro.2.title"), t!("intro.2.body")),
        _ => (t!("intro.3.title"), t!("intro.3.body")),
    };
    let text_color = ui.visuals().text_color();
    let g = p.layout(
        title.to_owned(),
        egui::FontId::proportional(22.0),
        text_color,
        area.width(),
    );
    let ty = illus.bottom() + 28.0;
    p.galley(
        pos2(area.center().x - g.size().x / 2.0, ty),
        g.clone(),
        text_color,
    );
    let muted = theme::muted(ui);
    // Wrap from the logical text so Arabic lines break in reading order.
    let logical = photovault::i18n::logical_of(body).unwrap_or_else(|| body.to_owned());
    let font = egui::FontId::proportional(14.5);
    let rows = crate::ui::bidi::rows(ui, &logical, &font, area.width() - 20.0);
    let mut y = ty + g.size().y + 12.0;
    for (_, row) in rows {
        let gr = p.layout_no_wrap(row, font.clone(), muted);
        p.galley(
            pos2(area.center().x - gr.size().x / 2.0, y),
            gr.clone(),
            muted,
        );
        y += gr.size().y;
    }
}

fn photo_card(p: &egui::Painter, rect: Rect, colors: (Color32, Color32), border: Color32) {
    p.rect_filled(rect, CornerRadius::same(10), border);
    let inner = rect.shrink(6.0);
    p.rect_filled(inner, CornerRadius::same(6), colors.0);
    // A tiny landscape.
    let base = inner.bottom();
    let pts = vec![
        pos2(inner.left(), base),
        pos2(
            inner.left() + inner.width() * 0.4,
            inner.top() + inner.height() * 0.45,
        ),
        pos2(inner.left() + inner.width() * 0.75, base),
    ];
    p.add(egui::Shape::convex_polygon(pts, colors.1, Stroke::NONE));
    p.circle_filled(
        pos2(
            inner.right() - inner.width() * 0.22,
            inner.top() + inner.height() * 0.3,
        ),
        inner.height() * 0.1,
        Color32::from_rgb(0xFF, 0xF1, 0xC8),
    );
}

fn draw_folder(p: &egui::Painter, ui: &egui::Ui, r: Rect, accent: Color32, t: f32) {
    let back = Rect::from_min_size(pos2(r.left() + 20.0, r.top() + 50.0), vec2(180.0, 110.0));
    p.rect_filled(
        Rect::from_min_size(back.min - vec2(0.0, 14.0), vec2(70.0, 24.0)),
        CornerRadius::same(6),
        accent.gamma_multiply(0.75),
    );
    p.rect_filled(back, CornerRadius::same(12), accent.gamma_multiply(0.75));
    // Photos pop up out of the folder one after another.
    let palettes = [
        (
            Color32::from_rgb(0xA4, 0x8B, 0xE8),
            Color32::from_rgb(0x3F, 0x6B, 0x3A),
        ),
        (
            Color32::from_rgb(0x7F, 0xB8, 0xE6),
            Color32::from_rgb(0x8C, 0x5A, 0x3C),
        ),
        (
            Color32::from_rgb(0xF2, 0xB4, 0x8C),
            Color32::from_rgb(0x5B, 0x7F, 0x4B),
        ),
    ];
    let border = ui.visuals().window_fill;
    for (i, colors) in palettes.iter().enumerate() {
        let k = ease_out_back(((t * 1.6) - i as f32 * 0.25).clamp(0.0, 1.0));
        let x = back.left() + 22.0 + i as f32 * 46.0;
        let rise = 70.0 * k;
        let card = Rect::from_min_size(pos2(x, back.top() + 34.0 - rise), vec2(64.0, 52.0));
        photo_card(p, card, *colors, border);
    }
    let front = Rect::from_min_max(pos2(back.left(), back.top() + 34.0), back.max);
    p.rect_filled(front, CornerRadius::same(12), accent);
}

fn draw_notes(p: &egui::Painter, ui: &egui::Ui, r: Rect, accent: Color32, t: f32) {
    let paper = Rect::from_center_size(r.center(), vec2(170.0, 160.0));
    p.add(
        egui::epaint::Shadow {
            offset: [0, 6],
            blur: 16,
            spread: 0,
            color: Color32::from_black_alpha(40),
        }
        .as_shape(paper, CornerRadius::same(14)),
    );
    p.rect_filled(paper, CornerRadius::same(14), ui.visuals().extreme_bg_color);
    p.rect_stroke(
        paper,
        CornerRadius::same(14),
        Stroke::new(1.5, accent.gamma_multiply(0.6)),
        StrokeKind::Inside,
    );
    // Lines "write themselves" from the reading-start side.
    let line_w = [120.0, 100.0, 130.0, 80.0, 110.0];
    let rtl = crate::ui::rtl();
    for (i, w) in line_w.iter().enumerate() {
        let k = ((t * 1.8) - i as f32 * 0.22).clamp(0.0, 1.0);
        let y = paper.top() + 30.0 + i as f32 * 24.0;
        let len = w * ease_out(k);
        let (a, b) = if rtl {
            (paper.right() - 22.0, paper.right() - 22.0 - len)
        } else {
            (paper.left() + 22.0, paper.left() + 22.0 + len)
        };
        p.line_segment(
            [pos2(a, y), pos2(b, y)],
            Stroke::new(6.0, theme::muted(ui).gamma_multiply(0.55)),
        );
        if k > 0.0 && k < 1.0 {
            // A blinking caret at the end of the line being written.
            p.line_segment(
                [pos2(b, y - 9.0), pos2(b, y + 9.0)],
                Stroke::new(2.0, accent),
            );
        }
    }
    // Tag chip.
    let chip = Rect::from_min_size(
        pos2(
            paper.center().x - 30.0,
            paper.bottom() - 6.0 - 18.0 * ease_out_back(t),
        ),
        vec2(60.0, 22.0),
    );
    p.rect_filled(chip, CornerRadius::same(255), accent);
}

fn draw_backup(p: &egui::Painter, ui: &egui::Ui, r: Rect, accent: Color32, t: f32, start: Instant) {
    let c = r.center();
    // A zip "box" with a zipper.
    let boxr = Rect::from_center_size(c, vec2(120.0, 140.0));
    p.rect_filled(boxr, CornerRadius::same(16), accent.gamma_multiply(0.85));
    for i in 0..7 {
        let y = boxr.top() + 14.0 + i as f32 * 12.0;
        let k = ((t * 2.0) - i as f32 * 0.08).clamp(0.0, 1.0);
        let w = 10.0 * ease_out(k);
        let x = if i % 2 == 0 { c.x - w } else { c.x };
        p.rect_filled(
            Rect::from_min_size(pos2(x, y), vec2(w, 6.0)),
            CornerRadius::same(2),
            Color32::WHITE.gamma_multiply(0.85),
        );
    }
    p.text(
        pos2(c.x, boxr.bottom() - 22.0),
        egui::Align2::CENTER_CENTER,
        ".zip",
        egui::FontId::proportional(16.0),
        Color32::WHITE,
    );
    // Check badge pops in, then a soft pulse ring.
    let k = ease_out_back(((t - 0.45) * 2.2).clamp(0.0, 1.0));
    let badge_c = Pos2::new(boxr.right() - 4.0, boxr.top() + 8.0);
    if k > 0.0 {
        let pulse = (start.elapsed().as_secs_f32() * 1.4).fract();
        p.circle_stroke(
            badge_c,
            22.0 + pulse * 18.0,
            Stroke::new(2.0, theme::SUCCESS.gamma_multiply((1.0 - pulse) * 0.6)),
        );
        p.circle_filled(badge_c, 22.0 * k, theme::SUCCESS);
        let s = 9.0 * k;
        p.line_segment(
            [
                badge_c + Vec2::new(-s, 0.0),
                badge_c + Vec2::new(-s * 0.3, s * 0.7),
            ],
            Stroke::new(3.5, Color32::WHITE),
        );
        p.line_segment(
            [
                badge_c + Vec2::new(-s * 0.3, s * 0.7),
                badge_c + Vec2::new(s, -s * 0.6),
            ],
            Stroke::new(3.5, Color32::WHITE),
        );
        ui.ctx().request_repaint();
    }
}
