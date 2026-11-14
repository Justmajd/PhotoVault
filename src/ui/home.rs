//! Home view: backup reminder, "On This Day", recent collections, empty state.

use eframe::egui::{self, Rect, RichText, Sense};
use photovault::t;

use crate::app::{Action, PhotoVaultApp, today};
use crate::theme;
use crate::ui::{empty_state, photo_grid, section};

/// Gentle banner when the last backup is older than the configured interval.
pub fn backup_banner(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    if app.reminder_dismissed {
        return;
    }
    let Some(age) = app.backup_overdue() else {
        return;
    };
    let text = if age < 0 {
        t!("banner.never").to_owned()
    } else {
        t!("banner.old", days = age)
    };
    egui::Frame::new()
        .fill(theme::AMBER.gamma_multiply(0.16))
        .stroke(egui::Stroke::new(1.0, theme::AMBER.gamma_multiply(0.5)))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::symmetric(16, 10))
        .show(ui, |ui| {
            crate::ui::row(ui, |ui| {
                ui.label(RichText::new("⚠").color(theme::AMBER));
                ui.label(text);
                ui.with_layout(crate::ui::end_layout(), |ui| {
                    if ui.small_button(t!("banner.later")).clicked() {
                        app.reminder_dismissed = true;
                    }
                    if ui.small_button(t!("banner.backup_now")).clicked() {
                        app.actions.push(Action::Backup);
                    }
                });
            });
        });
    ui.add_space(12.0);
}

fn collection_card(
    app: &mut PhotoVaultApp,
    ui: &mut egui::Ui,
    id: uuid::Uuid,
    subtitle: Option<String>,
    index: usize,
) {
    let accent = app.accent();
    let Some(archive) = app.archive.as_ref() else {
        return;
    };
    let Some(entry) = archive.get(id) else { return };
    let dir = archive.root.join(&entry.rel_dir);
    let w = 200.0;
    let (slot, resp) = ui.allocate_exact_size(egui::vec2(w, w + 58.0), Sense::click());
    let anim = app.config.animations;
    // Staggered entrance (fade + rise) and a gentle lift on hover.
    let enter = crate::anim::stagger(ui.ctx(), app.view_changed_at, index, 0.045, 0.32, anim);
    let hover = ui.ctx().animate_bool_with_time(
        egui::Id::unique(("card_hover", id, index)),
        resp.hovered(),
        if anim { 0.15 } else { 0.0 },
    );
    let rect = slot.translate(egui::vec2(0.0, (1.0 - enter) * 16.0 - hover * 3.0));
    let alpha = enter;
    if alpha <= 0.0 {
        return;
    }
    let painter = ui.painter().clone().with_layer_id(ui.layer_id());
    let mut painter = painter;
    painter.set_opacity(alpha);
    // Soft shadow that grows on hover.
    let shadow = egui::epaint::Shadow {
        offset: [0, (4.0 + hover * 6.0) as i8],
        blur: (10.0 + hover * 14.0) as u8,
        spread: 0,
        color: egui::Color32::from_black_alpha(if ui.visuals().dark_mode { 70 } else { 22 }),
    };
    painter.add(shadow.as_shape(rect, egui::CornerRadius::same(theme::CARD_RADIUS)));
    let fill = crate::anim::lerp_color(
        theme::card_fill(ui),
        ui.visuals().widgets.hovered.bg_fill,
        hover * 0.6,
    );
    painter.rect_filled(rect, egui::CornerRadius::same(theme::CARD_RADIUS), fill);
    let img = Rect::from_min_size(
        rect.min + egui::vec2(8.0, 8.0),
        egui::vec2(w - 16.0, w - 16.0),
    );
    {
        // Thumbnails are painted through the Ui; fade them with the card.
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(img));
        child.set_opacity(alpha);
        photo_grid::paint_cover(&child, &mut app.thumbs, &dir, &entry.data, img, 10, accent);
    }
    let p = painter.with_clip_rect(rect.shrink(8.0));
    let title = if entry.data.title.is_empty() {
        t!("collection.untitled").to_owned()
    } else {
        crate::ui::bidi::vis(&entry.data.title)
    };
    let (x, align) = if crate::ui::rtl() {
        (rect.right() - 12.0, egui::Align2::RIGHT_TOP)
    } else {
        (rect.left() + 12.0, egui::Align2::LEFT_TOP)
    };
    p.text(
        egui::pos2(x, img.bottom() + 10.0),
        align,
        title,
        egui::FontId::proportional(14.5),
        ui.visuals().text_color(),
    );
    let sub = subtitle.unwrap_or_else(|| {
        let date = entry.data.date.format("%Y-%m-%d").to_string();
        crate::ui::bidi::join(
            &[&date, &t!("sidebar.photos", n = entry.data.photos.len())],
            " · ",
        )
    });
    p.text(
        egui::pos2(x, img.bottom() + 30.0),
        align,
        sub,
        egui::FontId::proportional(12.0),
        theme::muted(ui),
    );
    if entry.data.favorite {
        p.text(
            img.right_top() + egui::vec2(-8.0, 8.0),
            egui::Align2::RIGHT_TOP,
            "★",
            egui::FontId::proportional(18.0),
            theme::AMBER,
        );
    }
    if resp.clicked() {
        app.actions.push(Action::Select(id));
    }
}

pub fn show(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    let accent = app.accent();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(28, 22))
                .show(ui, |ui| {
                    backup_banner(app, ui);
                    let Some(archive) = app.archive.as_ref() else {
                        return;
                    };
                    if archive.collections.is_empty() && archive.last_scan.unadopted.is_empty() {
                        empty_state(ui, accent, t!("home.empty_title"), t!("home.empty_body"));
                        ui.vertical_centered(|ui| {
                            ui.add_space(12.0);
                            crate::ui::row(ui, |ui| {
                                ui.add_space((ui.available_width() - 330.0).max(0.0) / 2.0);
                                if theme::primary_button(ui, accent, t!("top.new_collection"))
                                    .clicked()
                                {
                                    app.actions.push(Action::NewCollection);
                                }
                                if ui.button(t!("top.import_folder")).clicked() {
                                    app.actions.push(Action::ImportFolder);
                                }
                            });
                        });
                        return;
                    }

                    // Time-of-day greeting that fades in above the archive name.
                    let greet_t = crate::anim::stagger(
                        ui.ctx(),
                        app.view_changed_at,
                        0,
                        0.0,
                        0.5,
                        app.config.animations,
                    );
                    ui.scope(|ui| {
                        ui.multiply_opacity(greet_t);
                        let hour = chrono::Timelike::hour(&chrono::Local::now());
                        let greet = match hour {
                            5..=11 => t!("greet.morning"),
                            12..=16 => t!("greet.afternoon"),
                            17..=21 => t!("greet.evening"),
                            _ => t!("greet.night"),
                        };
                        ui.label(RichText::new(greet).size(16.0).color(app_accent(ui)));
                    });
                    ui.label(
                        RichText::new(crate::ui::bidi::vis(&archive.meta.name))
                            .text_style(theme::title_style())
                            .strong(),
                    );
                    ui.label(
                        RichText::new(t!(
                            "home.summary",
                            collections = archive.collections.len(),
                            photos = archive.photo_count()
                        ))
                        .color(theme::muted(ui)),
                    );
                    ui.add_space(16.0);

                    // Folders with photos but no collection.json.
                    let unadopted = archive.last_scan.unadopted.clone();
                    if !unadopted.is_empty() {
                        theme::card(ui).show(ui, |ui| {
                            section(ui, &t!("home.unadopted", n = unadopted.len()));
                            let c = theme::muted(ui);
                            crate::ui::bidi::para(ui, t!("home.unadopted_body"), 14.5, Some(c));
                            for rel in unadopted.iter().take(10) {
                                crate::ui::row(ui, |ui| {
                                    ui.label(rel.display().to_string());
                                    if ui.small_button(t!("home.adopt")).clicked() {
                                        app.actions.push(Action::AdoptFolder(rel.clone()));
                                    }
                                });
                            }
                        });
                        ui.add_space(16.0);
                    }

                    // On This Day.
                    let otd = photovault::stats::on_this_day(archive, today());
                    if !otd.is_empty() {
                        section(ui, t!("home.on_this_day"));
                        ui.add_space(6.0);
                        crate::ui::row_wrapped(ui, |ui| {
                            for (i, (id, years)) in otd.into_iter().enumerate() {
                                let sub = if years == 1 {
                                    t!("home.year_ago").to_owned()
                                } else {
                                    t!("home.years_ago", n = years)
                                };
                                collection_card(app, ui, id, Some(sub), i);
                            }
                        });
                        ui.add_space(18.0);
                    }

                    let Some(archive) = app.archive.as_ref() else {
                        return;
                    };
                    let mut recent: Vec<_> = archive
                        .collections
                        .iter()
                        .map(|c| (c.data.updated_at, c.data.id))
                        .collect();
                    recent.sort_by_key(|r| std::cmp::Reverse(r.0));
                    let recent: Vec<_> = recent.into_iter().take(8).map(|(_, id)| id).collect();
                    let favorites: Vec<_> = archive
                        .collections
                        .iter()
                        .filter(|c| c.data.favorite)
                        .take(8)
                        .map(|c| c.data.id)
                        .collect();
                    section(ui, t!("home.recent"));
                    ui.add_space(6.0);
                    crate::ui::row_wrapped(ui, |ui| {
                        for (i, id) in recent.into_iter().enumerate() {
                            collection_card(app, ui, id, None, i + 2);
                        }
                    });
                    if !favorites.is_empty() {
                        ui.add_space(18.0);
                        section(ui, t!("home.favorites"));
                        ui.add_space(6.0);
                        crate::ui::row_wrapped(ui, |ui| {
                            for (i, id) in favorites.into_iter().enumerate() {
                                collection_card(app, ui, id, None, i + 6);
                            }
                        });
                    }
                    ui.add_space(18.0);
                    let c = theme::muted(ui);
                    crate::ui::bidi::para(ui, t!("home.tip"), 12.0, Some(c));
                });
        });
}

fn app_accent(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().hyperlink_color
}
