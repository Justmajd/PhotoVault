//! UI modules. Each module adds drawing functions for one part of the window.

pub mod bidi;
pub mod collection_view;
pub mod dialogs;
pub mod home;
pub mod intro;
pub mod photo_grid;
pub mod search;
pub mod settings;
pub mod sidebar;
pub mod slideshow;
pub mod stats;
pub mod toasts;
pub mod viewer;
pub mod welcome;

use eframe::egui::{self, Align, Color32, CornerRadius, Layout, RichText, Stroke};
use photovault::fsutil::human_bytes;
use photovault::t;

use crate::app::{Action, PhotoVaultApp, View};
use crate::theme;

/// The normal window: top bar, sidebar, status bar and the current view.
pub fn main_layout(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    egui::Panel::top("top_bar")
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::symmetric(14, 10)),
        )
        .show(ui, |ui| top_bar(app, ui));
    egui::Panel::bottom("status_bar")
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::symmetric(14, 6)),
        )
        .show(ui, |ui| status_bar(app, ui));
    let sidebar = if rtl() {
        egui::Panel::right("sidebar")
    } else {
        egui::Panel::left("sidebar")
    };
    let sidebar = sidebar
        .resizable(true)
        .default_size(app.config.sidebar_width)
        .min_size(220.0)
        .max_size(520.0)
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::symmetric(10, 10)),
        )
        .show(ui, |ui| {
            ui.with_layout(column_layout(), |ui| sidebar::show(app, ui))
                .inner
        });
    let width = sidebar.response.rect.width();
    if (width - app.config.sidebar_width).abs() > 1.0 && !ui.input(|i| i.pointer.any_down()) {
        app.config.sidebar_width = width;
        app.mark_config_dirty();
    }
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::page_bg(ui.visuals()))
                .inner_margin(egui::Margin::same(0)),
        )
        .show(ui, |ui| {
            // Fade + slight rise whenever the view changes.
            let t = crate::anim::view_transition(app, ui.ctx());
            if t < 1.0 {
                ui.multiply_opacity(t);
                ui.add_space((1.0 - t) * 14.0);
            }
            ui.with_layout(column_layout(), |ui| show_view(app, ui));
        });
}

fn show_view(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    match app.view {
        View::Home => home::show(app, ui),
        View::Collection(id) => {
            if app.archive.as_ref().and_then(|a| a.get(id)).is_some() {
                collection_view::show(app, ui, id);
            } else {
                app.view = View::Home;
            }
        }
        View::Search => search::show(app, ui),
        View::Stats => stats::show_stats(app, ui),
        View::Map => stats::show_map(app, ui),
        View::BidiTest => bidi_test(ui),
    }
}

fn top_bar(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    let accent = app.accent();
    crate::ui::row(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
        theme::paint_logo(ui.painter(), rect, accent);
        ui.label(RichText::new(t!("app.name")).strong().size(17.0));
        ui.add_space(6.0);
        let name = app
            .archive
            .as_ref()
            .map(|a| bidi::vis(&a.meta.name))
            .unwrap_or_default();
        ui.menu_button(
            RichText::new(format!("{name}  ⏷")).color(theme::muted(ui)),
            |ui| archive_menu(app, ui),
        );
        ui.add_space(12.0);

        let search_id = egui::Id::unique("global_search");
        let resp = ui.add(
            crate::ui::field(&mut app.search_query, false, 14.5)
                .id(search_id)
                .hint_text(crate::ui::hint(t!("search.hint"), 14.5))
                .desired_width((ui.available_width() * 0.32).clamp(160.0, 380.0)),
        );
        if app.focus_search {
            app.focus_search = false;
            resp.request_focus();
        }
        if resp.changed() {
            if app.search_query.trim().is_empty() {
                if app.view == View::Search {
                    app.actions.push(Action::ShowView(View::Home));
                }
            } else if app.view != View::Search {
                app.actions.push(Action::ShowView(View::Search));
            }
        }
        if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.search_query.clear();
            app.actions.push(Action::ShowView(View::Home));
        }

        ui.with_layout(crate::ui::end_layout(), |ui| {
            if ui.button("⚙").on_hover_text(t!("top.settings")).clicked() {
                app.actions.push(Action::OpenSettings);
            }
            let dark = ui.visuals().dark_mode;
            if ui
                .button(if dark { "☀" } else { "🌙" })
                .on_hover_text(t!("top.theme"))
                .clicked()
            {
                app.actions.push(Action::ToggleTheme);
            }
            if ui
                .button(t!("top.restore"))
                .on_hover_text(t!("top.restore_tip"))
                .clicked()
            {
                app.actions.push(Action::Restore);
            }
            if ui
                .button(t!("top.backup"))
                .on_hover_text(t!("top.backup_tip"))
                .clicked()
            {
                app.actions.push(Action::Backup);
            }
            ui.menu_button(t!("top.import"), |ui| {
                if ui.button(t!("top.import_files")).clicked() {
                    app.actions.push(Action::ImportFiles);
                    ui.close();
                }
                if ui.button(t!("top.import_folder")).clicked() {
                    app.actions.push(Action::ImportFolder);
                    ui.close();
                }
            });
            if theme::primary_button(ui, accent, t!("top.new_collection")).clicked() {
                app.actions.push(Action::NewCollection);
            }
        });
    });
}

fn archive_menu(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    ui.set_min_width(240.0);
    if ui.button(t!("menu.home")).clicked() {
        app.actions.push(Action::ShowView(View::Home));
        ui.close();
    }
    if ui.button(t!("menu.stats")).clicked() {
        app.actions.push(Action::ShowView(View::Stats));
        ui.close();
    }
    if ui.button(t!("menu.map")).clicked() {
        app.actions.push(Action::ShowView(View::Map));
        ui.close();
    }
    ui.separator();
    if ui.button(t!("menu.integrity")).clicked() {
        app.actions.push(Action::CheckIntegrity);
        ui.close();
    }
    if ui.button(t!("menu.rescan")).clicked() {
        app.actions.push(Action::Rescan);
        ui.close();
    }
    if ui.button(t!("menu.archive_settings")).clicked() {
        app.actions.push(Action::ArchiveSettings);
        ui.close();
    }
    if let Some(root) = app.archive.as_ref().map(|a| a.root.clone())
        && ui.button(t!("menu.open_archive_folder")).clicked()
    {
        app.actions.push(Action::Open(root));
        ui.close();
    }
    ui.separator();
    ui.label(
        RichText::new(t!("menu.recent"))
            .small()
            .color(theme::muted(ui)),
    );
    let current = app.archive.as_ref().map(|a| a.root.clone());
    for p in app.config.recent_archives.clone() {
        if Some(&p) == current.as_ref() {
            continue;
        }
        let label = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.display().to_string());
        if ui
            .button(label)
            .on_hover_text(p.display().to_string())
            .clicked()
        {
            app.actions.push(Action::OpenArchive(p));
            ui.close();
        }
    }
    if ui.button(t!("menu.switch")).clicked() {
        app.actions.push(Action::CloseArchive);
        ui.close();
    }
}

fn status_bar(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    let Some(archive) = app.archive.as_ref() else {
        return;
    };
    let muted = theme::muted(ui);
    crate::ui::row(ui, |ui| {
        ui.label(
            RichText::new(t!("status.size", size = human_bytes(archive.total_bytes())))
                .small()
                .color(muted),
        );
        ui.label(RichText::new("·").small().color(muted));
        ui.label(
            RichText::new(t!("status.collections", n = archive.collections.len()))
                .small()
                .color(muted),
        );
        ui.label(RichText::new("·").small().color(muted));
        ui.label(
            RichText::new(t!("status.photos", n = archive.photo_count()))
                .small()
                .color(muted),
        );
        if let Some(job) = app
            .jobs
            .running
            .iter()
            .find(|j| !j.modal && j.title != "trash")
        {
            ui.label(
                RichText::new(format!("· {}", job.title))
                    .small()
                    .color(muted),
            );
        }
        ui.with_layout(crate::ui::end_layout(), |ui| {
            let (text, old) = match archive.meta.last_backup_at {
                Some(t) => {
                    let age = (chrono::Utc::now() - t).num_days();
                    let local = t.with_timezone(&chrono::Local);
                    (
                        t!("status.last_backup", date = local.format("%Y-%m-%d %H:%M")),
                        age > 30,
                    )
                }
                None => (t!("status.never_backed_up").to_owned(), true),
            };
            let color = if old { theme::AMBER } else { muted };
            let resp = ui.add(
                egui::Label::new(RichText::new(text).small().color(color))
                    .sense(egui::Sense::click()),
            );
            if resp.on_hover_text(t!("status.backup_tip")).clicked() {
                app.actions.push(Action::Backup);
            }
        });
    });
}

/// Full-window hint shown while files are dragged over the window.
pub fn drop_overlay(app: &PhotoVaultApp, ctx: &egui::Context) {
    let rect = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("drop_overlay"),
    ));
    let accent = app.accent();
    painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_black_alpha(150));
    let inner = rect.shrink(40.0);
    painter.rect_stroke(
        inner,
        CornerRadius::same(24),
        Stroke::new(3.0, accent),
        egui::StrokeKind::Inside,
    );
    let target = match app
        .current_collection()
        .and_then(|id| app.archive.as_ref()?.get(id))
    {
        Some(c) => t!("drop.into", title = c.data.title),
        None => t!("drop.new").to_owned(),
    };
    painter.text(
        inner.center(),
        egui::Align2::CENTER_CENTER,
        target,
        egui::FontId::proportional(26.0),
        Color32::WHITE,
    );
}

/// Shows a centered empty-state illustration with a message.
pub fn empty_state(ui: &mut egui::Ui, accent: Color32, title: &str, body: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(40.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(180.0, 130.0), egui::Sense::hover());
        let p = ui.painter();
        let back = egui::Rect::from_center_size(
            rect.center() + egui::vec2(18.0, -10.0),
            egui::vec2(120.0, 90.0),
        );
        p.rect_filled(back, CornerRadius::same(14), accent.gamma_multiply(0.25));
        let front = egui::Rect::from_center_size(
            rect.center() + egui::vec2(-14.0, 8.0),
            egui::vec2(120.0, 90.0),
        );
        p.rect_filled(front, CornerRadius::same(14), ui.visuals().window_fill);
        p.rect_stroke(
            front,
            CornerRadius::same(14),
            Stroke::new(2.0, accent),
            egui::StrokeKind::Inside,
        );
        theme::paint_logo(p, front.shrink(18.0), accent);
        ui.add_space(14.0);
        ui.label(RichText::new(title).size(20.0).strong());
        ui.add_space(4.0);
        let c = theme::muted(ui);
        crate::ui::bidi::paragraphs_aligned(
            ui,
            &photovault::i18n::logical_of(body).unwrap_or_else(|| body.to_owned()),
            egui::FontId::proportional(14.5),
            c,
            rtl(),
            true,
        );
    });
}

/// Whether the UI is laid out right to left (Arabic).
pub fn rtl() -> bool {
    photovault::i18n::is_rtl()
}

/// A horizontal row that runs right-to-left in the Arabic UI.
pub fn row<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    if rtl() {
        let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), add)
    } else {
        ui.horizontal(add)
    }
}

/// A wrapping horizontal row that runs right-to-left in the Arabic UI.
pub fn row_wrapped<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    if rtl() {
        let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(
            size,
            Layout::right_to_left(Align::Center).with_main_wrap(true),
            add,
        )
    } else {
        ui.horizontal_wrapped(add)
    }
}

/// Layout that pushes widgets to the *end* of a row (right in LTR, left in RTL).
pub fn end_layout() -> Layout {
    if rtl() {
        Layout::left_to_right(Align::Center)
    } else {
        Layout::right_to_left(Align::Center)
    }
}

/// A single row whose widgets are pushed to the end side (right in LTR).
pub fn end_row<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y + 6.0);
    ui.allocate_ui_with_layout(size, end_layout(), add)
}

/// Vertical layout aligned to the reading start (right in RTL).
pub fn column_layout() -> Layout {
    Layout::top_down(if rtl() { Align::Max } else { Align::Min })
}

/// A segmented control with an animated sliding highlight. Returns `true`
/// when the value changed.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    id_salt: &str,
    current: &mut T,
    options: &[(T, &str)],
    accent: Color32,
) -> bool {
    let font = egui::FontId::proportional(13.5);
    let pad = egui::vec2(14.0, 6.0);
    let galleys: Vec<_> = options
        .iter()
        .map(|(_, label)| {
            ui.painter()
                .layout_no_wrap((*label).to_owned(), font.clone(), Color32::WHITE)
        })
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + pad.x * 2.0).collect();
    let h = galleys.iter().map(|g| g.size().y).fold(0.0, f32::max) + pad.y * 2.0;
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total, h + 4.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(
        rect,
        CornerRadius::same(255),
        ui.visuals().widgets.inactive.bg_fill,
    );
    // Segment rects in visual order (reversed in the RTL UI).
    let mut rects = Vec::with_capacity(options.len());
    let mut x = rect.left() + 2.0;
    let order: Vec<usize> = if rtl() {
        (0..options.len()).rev().collect()
    } else {
        (0..options.len()).collect()
    };
    let mut by_index = vec![egui::Rect::NOTHING; options.len()];
    for &i in &order {
        let r =
            egui::Rect::from_min_size(egui::pos2(x, rect.top() + 2.0), egui::vec2(widths[i], h));
        by_index[i] = r;
        x += widths[i];
    }
    rects.extend(by_index);
    let mut changed = false;
    let selected = options.iter().position(|(v, _)| v == current).unwrap_or(0);
    let id = egui::Id::unique(("segmented", id_salt));
    let anim = if photovault_animations(ui) { 0.18 } else { 0.0 };
    let sx = ui
        .ctx()
        .animate_value_with_time(id.with("x"), rects[selected].left(), anim);
    let sw = ui
        .ctx()
        .animate_value_with_time(id.with("w"), rects[selected].width(), anim);
    let pill = egui::Rect::from_min_size(egui::pos2(sx, rects[selected].top()), egui::vec2(sw, h));
    ui.painter()
        .rect_filled(pill, CornerRadius::same(255), accent);
    for (i, ((value, _), g)) in options.iter().zip(galleys).enumerate() {
        let r = rects[i];
        let resp = ui.interact(r, id.with(i), egui::Sense::click());
        let on = i == selected;
        let color = if on {
            Color32::WHITE
        } else if resp.hovered() {
            ui.visuals().strong_text_color()
        } else {
            ui.visuals().text_color()
        };
        let pos = r.center() - g.size() / 2.0;
        ui.painter().galley(pos, g, color);
        if resp.clicked() && !on {
            *current = *value;
            changed = true;
        }
    }
    changed
}

fn photovault_animations(ui: &egui::Ui) -> bool {
    ui.ctx()
        .data(|d| d.get_temp::<bool>(egui::Id::unique("animations_enabled")))
        .unwrap_or(true)
}

/// Horizontal layout in reading direction.
pub fn row_layout() -> Layout {
    if rtl() {
        Layout::right_to_left(Align::Center)
    } else {
        Layout::left_to_right(Align::Center)
    }
}

/// An iOS-style switch with an animated knob.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool, accent: Color32) -> egui::Response {
    let size = ui.spacing().interact_size.y * egui::vec2(1.8, 0.9);
    let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(
        resp.id,
        *on,
        if photovault_animations(ui) { 0.16 } else { 0.0 },
    );
    let radius = rect.height() / 2.0;
    let off = ui.visuals().widgets.inactive.bg_fill;
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(255),
        crate::anim::lerp_color(off, accent, t),
    );
    let (from, to) = if rtl() {
        (rect.right() - radius, rect.left() + radius)
    } else {
        (rect.left() + radius, rect.right() - radius)
    };
    let x = egui::lerp(from..=to, crate::anim::ease_out(t));
    ui.painter().circle_filled(
        egui::pos2(x, rect.center().y),
        radius * 0.78,
        Color32::WHITE,
    );
    resp
}

/// A text field using the Arabic-first editor font. Right-aligned when its
/// content (or, when empty, the UI language) is right-to-left.
pub fn field(text: &mut String, multiline: bool, size: f32) -> egui::TextEdit<'_> {
    let rtl = if text.trim().is_empty() {
        photovault::i18n::is_rtl()
    } else {
        photovault::bidi::is_rtl_paragraph(text.lines().next().unwrap_or(""))
    };
    let te = if multiline {
        egui::TextEdit::multiline(text)
    } else {
        egui::TextEdit::singleline(text)
    };
    te.font(theme::editor_font(size)).align(if rtl {
        egui::Align2::RIGHT_TOP
    } else {
        egui::Align2::LEFT_TOP
    })
}

/// Hint text for a [`field`] (rendered with the display font).
pub fn hint(text: &str, size: f32) -> RichText {
    RichText::new(text).font(egui::FontId::proportional(size))
}

/// Section label.
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .text_style(theme::subheading_style())
            .strong(),
    );
}

/// Developer view showing bidi rendering samples (`PHOTOVAULT_VIEW=bidi`).
fn bidi_test(ui: &mut egui::Ui) {
    let samples = [
        "رحلة البتراء",
        "عام 2025 كان جميلاً",
        "زرنا Petra في 12 نيسان (أبريل)",
        "Trip to البتراء in 2024",
        "٣ صور جديدة",
    ];
    egui::Frame::new().inner_margin(egui::Margin::same(24)).show(ui, |ui| {
        for s in samples {
            ui.label(RichText::new(bidi::vis(s)).size(20.0));
        }
        ui.separator();
        bidi::paragraphs(
            ui,
            "زرنا الخزنة في الصباح الباكر قبل وصول الزوار عام 2024، وكان الجو جميلاً جداً. ثم مشينا إلى الدير (Ad Deir) وصعدنا 800 درجة تقريباً.\nWe took 120 photos.",
            egui::FontId::proportional(16.0),
            ui.visuals().text_color(),
            false,
        );
    });
}
