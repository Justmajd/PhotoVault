//! Visual design: dark/light visuals, accent colors, spacing and fonts.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    TextStyle, Theme, ThemePreference, Visuals,
};
use photovault::config::{AppConfig, ThemeMode};

/// Accent presets: `(name, color)`.
pub const ACCENTS: [(&str, Color32); 6] = [
    ("Ocean", Color32::from_rgb(0x5B, 0x8D, 0xEF)),
    ("Violet", Color32::from_rgb(0x9B, 0x7B, 0xF0)),
    ("Teal", Color32::from_rgb(0x2E, 0xB8, 0xA6)),
    ("Sunset", Color32::from_rgb(0xF0, 0x8A, 0x4B)),
    ("Rose", Color32::from_rgb(0xE8, 0x6A, 0x92)),
    ("Sand", Color32::from_rgb(0xC9, 0xA2, 0x5F)),
];

/// Amber used for warnings (old backups, missing files).
pub const AMBER: Color32 = Color32::from_rgb(0xF2, 0xB1, 0x3C);
/// Red used for destructive actions.
pub const DANGER: Color32 = Color32::from_rgb(0xE5, 0x5B, 0x5B);
/// Green used for success.
pub const SUCCESS: Color32 = Color32::from_rgb(0x4C, 0xC3, 0x8A);

/// Corner radius used for cards.
pub const CARD_RADIUS: u8 = 14;

/// Accent color from the config.
pub fn accent(config: &AppConfig) -> Color32 {
    ACCENTS[config.accent.min(ACCENTS.len() - 1)].1
}

/// Registers the bundled Noto fonts.
///
/// Display text (`Proportional`) is **Latin-first**: egui starts a new
/// shaping run whenever the font changes, so every Arabic word becomes its
/// own right-to-left run while spaces, digits and Latin stay left-to-right.
/// Together with `photovault::bidi::visual_string` this gives correct
/// mixed-direction display. Text fields use the **Arabic-first** `editor`
/// family instead, so a typed Arabic sentence stays one right-to-left run.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "noto_sans".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSans-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "noto_sans_arabic".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansArabic-Regular.ttf"
        ))),
    );
    let defaults = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut display = vec!["noto_sans".to_owned(), "noto_sans_arabic".to_owned()];
    display.extend(defaults.iter().cloned());
    let mut editor = vec!["noto_sans_arabic".to_owned(), "noto_sans".to_owned()];
    editor.extend(defaults.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, display);
    fonts
        .families
        .insert(FontFamily::Name(EDITOR_FAMILY.into()), editor);
    let mono = fonts.families.entry(FontFamily::Monospace).or_default();
    mono.push("noto_sans".to_owned());
    mono.push("noto_sans_arabic".to_owned());
    ctx.set_fonts(fonts);
}

/// Font family used inside text fields (Arabic-first, see [`install_fonts`]).
pub const EDITOR_FAMILY: &str = "editor";

/// Font for text fields.
pub fn editor_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(EDITOR_FAMILY.into()))
}

fn base_visuals(dark: bool, accent: Color32) -> Visuals {
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let (bg, panel, card, extreme, text, weak, stroke) = if dark {
        (
            Color32::from_rgb(0x15, 0x16, 0x1A),
            Color32::from_rgb(0x1A, 0x1B, 0x20),
            Color32::from_rgb(0x22, 0x23, 0x29),
            Color32::from_rgb(0x10, 0x11, 0x14),
            Color32::from_rgb(0xE8, 0xE8, 0xEE),
            Color32::from_rgb(0x9A, 0x9B, 0xA6),
            Color32::from_rgb(0x2E, 0x30, 0x37),
        )
    } else {
        (
            Color32::from_rgb(0xF4, 0xF3, 0xF1),
            Color32::from_rgb(0xFA, 0xF9, 0xF7),
            Color32::from_rgb(0xFF, 0xFF, 0xFF),
            Color32::from_rgb(0xFF, 0xFF, 0xFF),
            Color32::from_rgb(0x1E, 0x1F, 0x24),
            Color32::from_rgb(0x6A, 0x6B, 0x75),
            Color32::from_rgb(0xE2, 0xE1, 0xDE),
        )
    };
    v.panel_fill = panel;
    v.window_fill = card;
    v.faint_bg_color = card;
    v.extreme_bg_color = extreme;
    v.code_bg_color = card;
    v.override_text_color = Some(text);
    v.weak_text_color = Some(weak);
    v.hyperlink_color = accent;
    v.warn_fg_color = AMBER;
    v.error_fg_color = DANGER;
    v.selection.bg_fill = accent.gamma_multiply(if dark { 0.45 } else { 0.30 });
    v.selection.stroke = Stroke::new(1.0, accent);
    v.window_corner_radius = CornerRadius::same(CARD_RADIUS);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_stroke = Stroke::new(1.0, stroke);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 40 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 90 } else { 30 }),
    };
    v.text_edit_bg_color = Some(if dark {
        extreme
    } else {
        Color32::from_rgb(0xF7, 0xF6, 0xF4)
    });
    let radius = CornerRadius::same(8);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = card;
    w.noninteractive.weak_bg_fill = card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, stroke);
    w.noninteractive.fg_stroke = Stroke::new(1.0, text);
    w.noninteractive.corner_radius = radius;
    let inactive_bg = if dark {
        Color32::from_rgb(0x2A, 0x2B, 0x32)
    } else {
        Color32::from_rgb(0xEC, 0xEB, 0xE8)
    };
    let hovered_bg = if dark {
        Color32::from_rgb(0x33, 0x35, 0x3D)
    } else {
        Color32::from_rgb(0xE2, 0xE1, 0xDD)
    };
    w.inactive.bg_fill = inactive_bg;
    w.inactive.weak_bg_fill = inactive_bg;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, text);
    w.inactive.corner_radius = radius;
    w.hovered.bg_fill = hovered_bg;
    w.hovered.weak_bg_fill = hovered_bg;
    w.hovered.bg_stroke = Stroke::new(1.0, accent.gamma_multiply(0.6));
    w.hovered.fg_stroke = Stroke::new(1.5, text);
    w.hovered.corner_radius = radius;
    w.active.bg_fill = accent.gamma_multiply(0.7);
    w.active.weak_bg_fill = accent.gamma_multiply(0.7);
    w.active.bg_stroke = Stroke::new(1.0, accent);
    w.active.fg_stroke = Stroke::new(2.0, text);
    w.active.corner_radius = radius;
    w.open = w.hovered;
    let _ = bg;
    v
}

/// Background color of the central area.
/// Background of the central area, derived from the panel color so that it
/// follows the animated theme cross-fade.
pub fn page_bg(visuals: &Visuals) -> Color32 {
    let p = visuals.panel_fill;
    let lum = (p.r() as f32 * 0.3 + p.g() as f32 * 0.59 + p.b() as f32 * 0.11) / 255.0;
    p.lerp_to_gamma(Color32::BLACK, 0.03 + 0.12 * (1.0 - lum))
}

/// The full visuals for one theme (used by the theme cross-fade).
pub fn visuals_for(dark: bool, accent: Color32) -> Visuals {
    base_visuals(dark, accent)
}

fn lerp_widget(
    a: &egui::style::WidgetVisuals,
    b: &egui::style::WidgetVisuals,
    t: f32,
) -> egui::style::WidgetVisuals {
    let mut w = *b;
    w.bg_fill = a.bg_fill.lerp_to_gamma(b.bg_fill, t);
    w.weak_bg_fill = a.weak_bg_fill.lerp_to_gamma(b.weak_bg_fill, t);
    w.bg_stroke.color = a.bg_stroke.color.lerp_to_gamma(b.bg_stroke.color, t);
    w.fg_stroke.color = a.fg_stroke.color.lerp_to_gamma(b.fg_stroke.color, t);
    w
}

/// Blends every color of two visuals (`t` = 0 → `a`, 1 → `b`).
pub fn lerp_visuals(a: &Visuals, b: &Visuals, t: f32) -> Visuals {
    let c = |x: Color32, y: Color32| x.lerp_to_gamma(y, t);
    let mut v = b.clone();
    v.dark_mode = if t < 0.5 { a.dark_mode } else { b.dark_mode };
    v.panel_fill = c(a.panel_fill, b.panel_fill);
    v.window_fill = c(a.window_fill, b.window_fill);
    v.faint_bg_color = c(a.faint_bg_color, b.faint_bg_color);
    v.extreme_bg_color = c(a.extreme_bg_color, b.extreme_bg_color);
    v.code_bg_color = c(a.code_bg_color, b.code_bg_color);
    v.window_stroke.color = c(a.window_stroke.color, b.window_stroke.color);
    v.override_text_color = Some(c(a.text_color(), b.text_color()));
    v.weak_text_color = Some(c(a.weak_text_color(), b.weak_text_color()));
    v.text_edit_bg_color = Some(c(a.text_edit_bg_color(), b.text_edit_bg_color()));
    v.selection.bg_fill = c(a.selection.bg_fill, b.selection.bg_fill);
    v.hyperlink_color = c(a.hyperlink_color, b.hyperlink_color);
    v.window_shadow.color = c(a.window_shadow.color, b.window_shadow.color);
    v.widgets.noninteractive = lerp_widget(&a.widgets.noninteractive, &b.widgets.noninteractive, t);
    v.widgets.inactive = lerp_widget(&a.widgets.inactive, &b.widgets.inactive, t);
    v.widgets.hovered = lerp_widget(&a.widgets.hovered, &b.widgets.hovered, t);
    v.widgets.active = lerp_widget(&a.widgets.active, &b.widgets.active, t);
    v.widgets.open = lerp_widget(&a.widgets.open, &b.widgets.open, t);
    v
}

/// Card fill color.
pub fn card_fill(ui: &egui::Ui) -> Color32 {
    ui.visuals().window_fill
}

/// Muted text color.
pub fn muted(ui: &egui::Ui) -> Color32 {
    ui.visuals().weak_text_color()
}

/// Applies theme, accent, spacing and text styles.
pub fn apply(ctx: &egui::Context, config: &AppConfig) {
    let accent = accent(config);
    ctx.set_visuals_of(Theme::Dark, base_visuals(true, accent));
    ctx.set_visuals_of(Theme::Light, base_visuals(false, accent));
    ctx.set_theme(match config.theme {
        ThemeMode::Dark => ThemePreference::Dark,
        ThemeMode::Light => ThemePreference::Light,
        ThemeMode::System => ThemePreference::System,
    });
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.window_margin = egui::Margin::same(20);
        style.spacing.menu_margin = egui::Margin::same(8);
        style.spacing.scroll.bar_width = 8.0;
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(12.0)),
            (TextStyle::Body, FontId::proportional(14.5)),
            (TextStyle::Button, FontId::proportional(14.0)),
            (TextStyle::Heading, FontId::proportional(22.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
            (TextStyle::Name("title".into()), FontId::proportional(28.0)),
            (
                TextStyle::Name("subheading".into()),
                FontId::proportional(16.5),
            ),
        ]
        .into();
        style.animation_time = 0.12;
    });
}

/// Large title text style.
pub fn title_style() -> TextStyle {
    TextStyle::Name("title".into())
}

/// Section heading text style.
pub fn subheading_style() -> TextStyle {
    TextStyle::Name("subheading".into())
}

/// A rounded card frame.
pub fn card(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(card_fill(ui))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(egui::Margin::same(18))
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
}

/// An accent-filled primary button.
pub fn primary_button(
    ui: &mut egui::Ui,
    accent: Color32,
    text: impl Into<String>,
) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(text.into())
                .color(Color32::WHITE)
                .strong(),
        )
        .fill(accent)
        .corner_radius(CornerRadius::same(8)),
    )
}

/// A red button for destructive actions.
pub fn danger_button(ui: &mut egui::Ui, enabled: bool, text: impl Into<String>) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(
            egui::RichText::new(text.into())
                .color(Color32::WHITE)
                .strong(),
        )
        .fill(DANGER)
        .corner_radius(CornerRadius::same(8)),
    )
}

/// Draws the PhotoVault logo mark (rounded square with mountains and sun).
pub fn paint_logo(painter: &egui::Painter, rect: egui::Rect, accent: Color32) {
    let r = rect.width().min(rect.height());
    let rect = egui::Rect::from_center_size(rect.center(), egui::vec2(r, r));
    painter.rect_filled(rect, CornerRadius::same((r * 0.24) as u8), accent);
    let inner = rect.shrink(r * 0.2);
    let white = Color32::WHITE;
    painter.circle_filled(
        inner.left_top() + egui::vec2(inner.width() * 0.72, inner.height() * 0.28),
        r * 0.09,
        white,
    );
    let base = inner.bottom();
    let pts = [
        egui::pos2(inner.left(), base),
        egui::pos2(
            inner.left() + inner.width() * 0.36,
            inner.top() + inner.height() * 0.38,
        ),
        egui::pos2(
            inner.left() + inner.width() * 0.56,
            inner.top() + inner.height() * 0.66,
        ),
        egui::pos2(
            inner.left() + inner.width() * 0.70,
            inner.top() + inner.height() * 0.52,
        ),
        egui::pos2(inner.right(), base),
    ];
    painter.add(egui::Shape::convex_polygon(
        pts[..3]
            .iter()
            .copied()
            .chain([egui::pos2(inner.left() + inner.width() * 0.56, base)])
            .collect(),
        white,
        Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![
            pts[2],
            pts[3],
            pts[4],
            egui::pos2(inner.left() + inner.width() * 0.5, base),
        ],
        white.gamma_multiply(0.8),
        Stroke::NONE,
    ));
}
