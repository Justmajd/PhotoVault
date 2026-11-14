//! Left sidebar: filter chips and the Year → Month timeline.

use chrono::Datelike;
use eframe::egui::{self, Color32, CornerRadius, Rect, RichText, Sense};
use photovault::t;

use crate::app::{Action, PhotoVaultApp, SidebarFilter, View};
use crate::theme;
use crate::ui::photo_grid;

fn chip(ui: &mut egui::Ui, selected: bool, text: &str, accent: Color32) -> egui::Response {
    let (fill, color) = if selected {
        (accent, Color32::WHITE)
    } else {
        (
            ui.visuals().widgets.inactive.bg_fill,
            ui.visuals().text_color(),
        )
    };
    ui.add(
        egui::Button::new(RichText::new(text).size(12.5).color(color))
            .fill(fill)
            .corner_radius(CornerRadius::same(255))
            .min_size(egui::vec2(0.0, 24.0)),
    )
}

/// Month name for the timeline.
pub fn month_name(m: u32) -> &'static str {
    match m {
        1 => t!("month.1"),
        2 => t!("month.2"),
        3 => t!("month.3"),
        4 => t!("month.4"),
        5 => t!("month.5"),
        6 => t!("month.6"),
        7 => t!("month.7"),
        8 => t!("month.8"),
        9 => t!("month.9"),
        10 => t!("month.10"),
        11 => t!("month.11"),
        _ => t!("month.12"),
    }
}

pub fn show(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    let rtl = crate::ui::rtl();
    let accent = app.accent();
    let Some(archive) = app.archive.as_ref() else {
        return;
    };
    let tags = archive.all_tags();

    crate::ui::row_wrapped(ui, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        if chip(
            ui,
            app.sidebar_filter == SidebarFilter::All,
            t!("sidebar.all"),
            accent,
        )
        .clicked()
        {
            app.sidebar_filter = SidebarFilter::All;
        }
        if chip(
            ui,
            app.sidebar_filter == SidebarFilter::Favorites,
            t!("sidebar.favorites"),
            accent,
        )
        .clicked()
        {
            app.sidebar_filter = SidebarFilter::Favorites;
        }
        for (tag, _) in tags.iter().take(12) {
            let sel = app.sidebar_filter == SidebarFilter::Tag(tag.clone());
            if chip(ui, sel, &crate::ui::bidi::vis(&format!("#{tag}")), accent).clicked() {
                app.sidebar_filter = if sel {
                    SidebarFilter::All
                } else {
                    SidebarFilter::Tag(tag.clone())
                };
            }
        }
    });
    ui.add_space(6.0);
    ui.separator();

    let selected = match app.view {
        View::Collection(id) => Some(id),
        _ => None,
    };
    let filter = app.sidebar_filter.clone();
    let entries: Vec<_> = archive
        .collections
        .iter()
        .filter(|c| match &filter {
            SidebarFilter::All => true,
            SidebarFilter::Favorites => c.data.favorite,
            SidebarFilter::Tag(t) => c.data.tags.contains(t),
        })
        .collect();

    if entries.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(if archive.collections.is_empty() {
                    t!("sidebar.empty")
                } else {
                    t!("sidebar.no_match")
                })
                .color(theme::muted(ui)),
            );
        });
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut current_year = None;
            let mut current_month = None;
            let mut year_open = true;
            for entry in entries {
                let d = &entry.data;
                let year = d.date.year();
                if current_year != Some(year) {
                    current_year = Some(year);
                    current_month = None;
                    year_open = !app.collapsed_years.contains(&year);
                    ui.add_space(4.0);
                    let arrow = if year_open { "⏷" } else { "⏵" };
                    let resp = ui.add(
                        egui::Label::new(
                            RichText::new(format!("{arrow}  {year}"))
                                .strong()
                                .size(15.0),
                        )
                        .sense(Sense::click()),
                    );
                    if resp.clicked() {
                        if year_open {
                            app.collapsed_years.insert(year);
                        } else {
                            app.collapsed_years.remove(&year);
                        }
                    }
                }
                if !year_open {
                    continue;
                }
                if current_month != Some(d.date.month()) {
                    current_month = Some(d.date.month());
                    ui.label(
                        RichText::new(month_name(d.date.month()))
                            .small()
                            .color(theme::muted(ui)),
                    );
                }
                let is_sel = selected == Some(d.id);
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 54.0), Sense::click());
                // Smoothly animated hover and selection highlights.
                let anim = app.config.animations;
                let dur = if anim { 0.16 } else { 0.0 };
                let sel_t = ui.ctx().animate_bool_with_time(
                    egui::Id::unique(("sb_sel", d.id)),
                    is_sel,
                    dur,
                );
                let hov_t = ui.ctx().animate_bool_with_time(
                    egui::Id::unique(("sb_hov", d.id)),
                    resp.hovered(),
                    dur,
                );
                if hov_t > 0.0 {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(10),
                        ui.visuals()
                            .widgets
                            .hovered
                            .bg_fill
                            .gamma_multiply(0.6 * hov_t),
                    );
                }
                if sel_t > 0.0 {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(10),
                        accent.gamma_multiply(0.22 * sel_t),
                    );
                    // Accent bar on the reading-start edge.
                    let bar_x = if rtl { rect.right() - 3.0 } else { rect.left() };
                    let bar = Rect::from_min_size(
                        egui::pos2(bar_x, rect.top() + 12.0),
                        egui::vec2(3.0, 30.0 * sel_t),
                    );
                    ui.painter().rect_filled(bar, CornerRadius::same(2), accent);
                }
                // Thumbnail lifts slightly on hover.
                let lift = hov_t * 1.5;
                let thumb_x = if rtl {
                    rect.right() - 48.0
                } else {
                    rect.left() + 6.0
                };
                let thumb = Rect::from_min_size(
                    egui::pos2(thumb_x, rect.top() + 6.0 - lift),
                    egui::vec2(42.0, 42.0),
                );
                let dir = archive.root.join(&entry.rel_dir);
                photo_grid::paint_cover(ui, &mut app.thumbs, &dir, d, thumb, 8, accent);
                let (text_x, align, clip) = if rtl {
                    let x = thumb.left() - 10.0;
                    (
                        x,
                        egui::Align2::RIGHT_TOP,
                        Rect::from_min_max(
                            egui::pos2(rect.left() + 22.0, rect.top()),
                            egui::pos2(x, rect.bottom()),
                        ),
                    )
                } else {
                    let x = thumb.right() + 10.0;
                    (
                        x,
                        egui::Align2::LEFT_TOP,
                        Rect::from_min_max(
                            egui::pos2(x, rect.top()),
                            egui::pos2(rect.right() - 22.0, rect.bottom()),
                        ),
                    )
                };
                let painter = ui.painter().with_clip_rect(clip);
                let title = if d.title.is_empty() {
                    t!("collection.untitled").to_owned()
                } else {
                    crate::ui::bidi::vis(&d.title)
                };
                painter.text(
                    egui::pos2(text_x, rect.top() + 9.0),
                    align,
                    title,
                    egui::FontId::proportional(14.0),
                    ui.visuals().text_color(),
                );
                let date = d.date.format("%Y-%m-%d").to_string();
                let sub = crate::ui::bidi::join(
                    &[&date, &t!("sidebar.photos", n = d.photos.len())],
                    " · ",
                );
                painter.text(
                    egui::pos2(text_x, rect.top() + 31.0),
                    align,
                    sub,
                    egui::FontId::proportional(12.0),
                    theme::muted(ui),
                );
                let end_x = if rtl {
                    rect.left() + 8.0
                } else {
                    rect.right() - 8.0
                };
                if d.favorite {
                    let a = if rtl {
                        egui::Align2::LEFT_CENTER
                    } else {
                        egui::Align2::RIGHT_CENTER
                    };
                    ui.painter().text(
                        egui::pos2(end_x, rect.center().y),
                        a,
                        "★",
                        egui::FontId::proportional(14.0),
                        theme::AMBER,
                    );
                }
                if !entry.missing.is_empty() {
                    ui.painter().circle_filled(
                        egui::pos2(end_x, rect.top() + 10.0),
                        3.5,
                        theme::AMBER,
                    );
                }
                if resp.clicked() {
                    app.actions.push(Action::Select(d.id));
                }
                let id = d.id;
                resp.context_menu(|ui| {
                    if ui.button(t!("actions.add_photos")).clicked() {
                        app.actions.push(Action::AddPhotos(id));
                        ui.close();
                    }
                    if ui.button(t!("actions.open_folder")).clicked() {
                        app.actions.push(Action::Open(dir.clone()));
                        ui.close();
                    }
                    if ui.button(t!("actions.slideshow")).clicked() {
                        app.actions.push(Action::Slideshow(id));
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(RichText::new(t!("actions.delete_collection")).color(theme::DANGER))
                        .clicked()
                    {
                        app.actions.push(Action::AskDeleteCollection(id));
                        ui.close();
                    }
                });
            }
        });
}
