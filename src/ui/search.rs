//! Global search results with highlighted snippets and filters.

use eframe::egui::{self, RichText, Sense, TextFormat, text::LayoutJob};
use photovault::search::{self, MatchField};
use photovault::t;

use crate::app::{Action, PhotoVaultApp, from_jiff};
use crate::theme;

pub fn show(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    let accent = app.accent();
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(28, 22))
        .show(ui, |ui| {
            ui.label(
                RichText::new(t!("search.title"))
                    .text_style(theme::title_style())
                    .strong(),
            );
            ui.add_space(6.0);
            // Filters.
            crate::ui::row_wrapped(ui, |ui| {
                ui.checkbox(&mut app.search_use_from, t!("search.from"));
                ui.add_enabled_ui(app.search_use_from, |ui| {
                    ui.add(
                        egui_extras::DatePickerButton::new(&mut app.search_from)
                            .id_salt("search_from"),
                    );
                });
                ui.checkbox(&mut app.search_use_to, t!("search.to"));
                ui.add_enabled_ui(app.search_use_to, |ui| {
                    ui.add(
                        egui_extras::DatePickerButton::new(&mut app.search_to).id_salt("search_to"),
                    );
                });
                ui.checkbox(
                    &mut app.search_filters.only_favorites,
                    t!("search.favorites"),
                );
                let tags = app
                    .archive
                    .as_ref()
                    .map(|a| a.all_tags())
                    .unwrap_or_default();
                let current = app
                    .search_filters
                    .tag
                    .clone()
                    .unwrap_or_else(|| t!("search.any_tag").to_owned());
                egui::ComboBox::from_id_salt("search_tag")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.search_filters.tag,
                            None,
                            t!("search.any_tag"),
                        );
                        for (tag, n) in tags {
                            ui.selectable_value(
                                &mut app.search_filters.tag,
                                Some(tag.clone()),
                                crate::ui::bidi::vis(&format!("#{tag} ({n})")),
                            );
                        }
                    });
            });
            app.search_filters.from = app.search_use_from.then(|| from_jiff(app.search_from));
            app.search_filters.to = app.search_use_to.then(|| from_jiff(app.search_to));
            ui.add_space(10.0);

            let Some(archive) = app.archive.as_ref() else {
                return;
            };
            let fresh = match &app.search_cache {
                Some((q, f, g, _)) => {
                    q != &app.search_query || f != &app.search_filters || *g != app.generation
                }
                None => true,
            };
            if fresh {
                let hits = search::search(archive, &app.search_query, &app.search_filters, 500);
                app.search_cache = Some((
                    app.search_query.clone(),
                    app.search_filters.clone(),
                    app.generation,
                    hits,
                ));
            }
            let hits = app
                .search_cache
                .as_ref()
                .map(|c| c.3.clone())
                .unwrap_or_default();
            ui.label(RichText::new(t!("search.count", n = hits.len())).color(theme::muted(ui)));
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for hit in hits {
                        let field = match &hit.field {
                            MatchField::Title => t!("search.field_title").to_owned(),
                            MatchField::Notes => t!("search.field_notes").to_owned(),
                            MatchField::Tag => t!("search.field_tag").to_owned(),
                            MatchField::Caption(f) => t!("search.field_caption", file = f),
                            MatchField::FileName(_) => t!("search.field_file").to_owned(),
                        };
                        let resp = theme::card(ui)
                            .inner_margin(egui::Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                crate::ui::row(ui, |ui| {
                                    ui.label(
                                        RichText::new(crate::ui::bidi::vis(&hit.collection_title))
                                            .strong(),
                                    );
                                    ui.label(
                                        RichText::new(hit.date.format("%Y-%m-%d").to_string())
                                            .small()
                                            .color(theme::muted(ui)),
                                    );
                                    ui.label(RichText::new(field).small().color(accent));
                                });
                                let mut job = LayoutJob::default();
                                let normal = TextFormat {
                                    font_id: egui::FontId::proportional(13.5),
                                    color: ui.visuals().text_color(),
                                    ..Default::default()
                                };
                                let strong = TextFormat {
                                    background: accent.gamma_multiply(0.35),
                                    color: ui.visuals().strong_text_color(),
                                    ..normal.clone()
                                };
                                let (a, b) = hit.highlight;
                                let s = &hit.snippet;
                                if a <= b
                                    && b <= s.len()
                                    && s.is_char_boundary(a)
                                    && s.is_char_boundary(b)
                                {
                                    job = crate::ui::bidi::highlight_job(s, (a, b), normal, strong);
                                } else {
                                    job.append(&crate::ui::bidi::vis(s), 0.0, normal);
                                }
                                ui.label(job);
                            })
                            .response
                            .interact(Sense::click());
                        if resp.clicked() {
                            app.actions.push(Action::Select(hit.collection_id));
                            if let MatchField::Caption(f) | MatchField::FileName(f) = &hit.field
                                && let Some(idx) = archive
                                    .get(hit.collection_id)
                                    .and_then(|c| c.data.photo_index(f))
                            {
                                app.actions.push(Action::OpenViewer {
                                    id: hit.collection_id,
                                    index: idx,
                                });
                            }
                        }
                        ui.add_space(4.0);
                    }
                });
        });
}
