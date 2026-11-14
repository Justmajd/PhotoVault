//! Statistics page (bar chart drawn with the painter) and the GPS photo map list.

use eframe::egui::{self, Rect, RichText};
use photovault::fsutil::human_bytes;
use photovault::t;

use crate::app::{Action, PhotoVaultApp};
use crate::theme;
use crate::ui::section;

fn stat_tile(ui: &mut egui::Ui, value: String, label: &str) {
    theme::card(ui).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.set_min_width(150.0);
            ui.label(RichText::new(value).size(24.0).strong());
            ui.label(RichText::new(label).small().color(theme::muted(ui)));
        });
    });
}

pub fn show_stats(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    let accent = app.accent();
    let Some(archive) = app.archive.as_ref() else {
        return;
    };
    let s = photovault::stats::compute(archive);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(28, 22))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(t!("stats.title"))
                            .text_style(theme::title_style())
                            .strong(),
                    );
                    ui.add_space(10.0);
                    crate::ui::row_wrapped(ui, |ui| {
                        stat_tile(ui, s.collections.to_string(), t!("stats.collections"));
                        stat_tile(ui, s.photos.to_string(), t!("stats.photos"));
                        stat_tile(ui, human_bytes(s.total_bytes), t!("stats.size"));
                        stat_tile(ui, s.favorites.to_string(), t!("stats.favorites"));
                    });
                    ui.add_space(16.0);
                    theme::card(ui).show(ui, |ui| {
                        section(ui, t!("stats.per_year"));
                        ui.add_space(8.0);
                        let years: Vec<(i32, usize)> =
                            s.photos_per_year.iter().map(|(y, n)| (*y, *n)).collect();
                        if years.is_empty() {
                            ui.label(RichText::new(t!("stats.no_data")).color(theme::muted(ui)));
                            return;
                        }
                        let max = years.iter().map(|(_, n)| *n).max().unwrap_or(1).max(1) as f32;
                        let h = 180.0;
                        let w = ui.available_width();
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(w, h + 36.0), egui::Sense::hover());
                        let bar_area = Rect::from_min_size(rect.min, egui::vec2(w, h));
                        let slot = (w / years.len() as f32).min(90.0);
                        let bar_w = (slot * 0.62).max(6.0);
                        let p = ui.painter();
                        p.line_segment(
                            [bar_area.left_bottom(), bar_area.right_bottom()],
                            egui::Stroke::new(
                                1.0,
                                ui.visuals().widgets.noninteractive.bg_stroke.color,
                            ),
                        );
                        for (i, (year, n)) in years.iter().enumerate() {
                            let x = bar_area.left() + slot * i as f32 + (slot - bar_w) / 2.0;
                            let bh = (*n as f32 / max) * (h - 20.0);
                            let bar = Rect::from_min_max(
                                egui::pos2(x, bar_area.bottom() - bh),
                                egui::pos2(x + bar_w, bar_area.bottom()),
                            );
                            p.rect_filled(
                                bar,
                                egui::CornerRadius {
                                    nw: 6,
                                    ne: 6,
                                    sw: 0,
                                    se: 0,
                                },
                                accent,
                            );
                            p.text(
                                bar.center_top() - egui::vec2(0.0, 4.0),
                                egui::Align2::CENTER_BOTTOM,
                                n.to_string(),
                                egui::FontId::proportional(11.0),
                                theme::muted(ui),
                            );
                            p.text(
                                egui::pos2(bar.center().x, bar_area.bottom() + 6.0),
                                egui::Align2::CENTER_TOP,
                                year.to_string(),
                                egui::FontId::proportional(12.0),
                                ui.visuals().text_color(),
                            );
                        }
                    });
                    ui.add_space(16.0);
                    ui.columns(2, |cols| {
                        theme::card(&cols[0]).show(&mut cols[0], |ui| {
                            ui.set_width(ui.available_width());
                            section(ui, t!("stats.largest"));
                            for (title, id, n, bytes) in &s.largest {
                                crate::ui::row(ui, |ui| {
                                    if ui.link(crate::ui::bidi::vis(title)).clicked() {
                                        app.actions.push(Action::Select(*id));
                                    }
                                    ui.label(
                                        RichText::new(crate::ui::bidi::join(
                                            &[&t!("sidebar.photos", n = n), &human_bytes(*bytes)],
                                            " · ",
                                        ))
                                        .small()
                                        .color(theme::muted(ui)),
                                    );
                                });
                            }
                        });
                        theme::card(&cols[1]).show(&mut cols[1], |ui| {
                            ui.set_width(ui.available_width());
                            section(ui, t!("stats.cameras"));
                            if s.cameras.is_empty() {
                                ui.label(
                                    RichText::new(t!("stats.no_cameras")).color(theme::muted(ui)),
                                );
                            }
                            for (cam, n) in &s.cameras {
                                crate::ui::row(ui, |ui| {
                                    ui.label(crate::ui::bidi::vis(cam));
                                    ui.label(
                                        RichText::new(t!("sidebar.photos", n = n))
                                            .small()
                                            .color(theme::muted(ui)),
                                    );
                                });
                            }
                        });
                    });
                });
        });
}

pub fn show_map(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    let Some(archive) = app.archive.as_ref() else {
        return;
    };
    let entries = photovault::stats::map_entries(archive);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(28, 22))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(t!("map.title"))
                            .text_style(theme::title_style())
                            .strong(),
                    );
                    let c = theme::muted(ui);
                    crate::ui::bidi::para(ui, t!("map.subtitle"), 14.5, Some(c));
                    ui.add_space(12.0);
                    if entries.is_empty() {
                        ui.label(RichText::new(t!("map.empty")).color(theme::muted(ui)));
                    }
                    for e in entries {
                        theme::card(ui)
                            .inner_margin(egui::Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                crate::ui::row(ui, |ui| {
                                    ui.label("📍");
                                    if ui
                                        .link(
                                            RichText::new(crate::ui::bidi::vis(&e.title)).strong(),
                                        )
                                        .clicked()
                                    {
                                        app.actions.push(Action::Select(e.collection_id));
                                    }
                                    ui.label(
                                        RichText::new(e.date.format("%Y-%m-%d").to_string())
                                            .small()
                                            .color(theme::muted(ui)),
                                    );
                                    ui.label(
                                        RichText::new(t!("map.with_gps", n = e.photos_with_gps))
                                            .small()
                                            .color(theme::muted(ui)),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{:.4}, {:.4}",
                                            e.center.lat, e.center.lon
                                        ))
                                        .small()
                                        .monospace(),
                                    );
                                    ui.with_layout(crate::ui::end_layout(), |ui| {
                                        if ui.button(t!("map.open_osm")).clicked() {
                                            app.actions.push(Action::OpenUrl(e.center.osm_url()));
                                        }
                                    });
                                });
                            });
                        ui.add_space(4.0);
                    }
                });
        });
}
