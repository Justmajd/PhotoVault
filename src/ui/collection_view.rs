//! Center panel for a selected collection: title, date, tags, notes and photos.

use eframe::egui::{self, Color32, CornerRadius, RichText};
use photovault::t;
use uuid::Uuid;

use crate::app::{Action, PhotoVaultApp, from_jiff, to_jiff};
use crate::theme;
use crate::ui::{empty_state, photo_grid};

pub fn show(app: &mut PhotoVaultApp, ui: &mut egui::Ui, id: Uuid) {
    let accent = app.accent();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt(("collection_scroll", id))
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(28, 22))
                .show(ui, |ui| {
                    crate::ui::home::backup_banner(app, ui);
                    header(app, ui, id, accent);
                    ui.add_space(14.0);
                    notes_card(app, ui, id);
                    ui.add_space(14.0);
                    actions_bar(app, ui, id, accent);
                    ui.add_space(10.0);
                    let empty = app
                        .archive
                        .as_ref()
                        .and_then(|a| a.get(id))
                        .is_none_or(|c| c.data.photos.is_empty());
                    if empty {
                        empty_state(
                            ui,
                            accent,
                            t!("collection.empty_title"),
                            t!("collection.empty_body"),
                        );
                        ui.vertical_centered(|ui| {
                            ui.add_space(10.0);
                            if theme::primary_button(ui, accent, t!("actions.add_photos")).clicked()
                            {
                                app.actions.push(Action::AddPhotos(id));
                            }
                        });
                    } else {
                        photo_grid::show(app, ui, id);
                    }
                });
        });
}

fn header(app: &mut PhotoVaultApp, ui: &mut egui::Ui, id: Uuid, accent: Color32) {
    let Some(archive) = app.archive.as_mut() else {
        return;
    };
    let Some(entry) = archive.get_mut(id) else {
        return;
    };
    let mut dirty = false;
    let mut sync = false;

    crate::ui::row(ui, |ui| {
        let star = if entry.data.favorite { "★" } else { "☆" };
        let star_color = if entry.data.favorite {
            theme::AMBER
        } else {
            theme::muted(ui)
        };
        if ui
            .add(egui::Button::new(RichText::new(star).size(24.0).color(star_color)).frame(false))
            .on_hover_text(t!("collection.favorite"))
            .clicked()
        {
            entry.data.favorite = !entry.data.favorite;
            dirty = true;
        }
        let rtl_title = photovault::bidi::has_rtl(&entry.data.title);
        if rtl_title && !app.edit.title_editing {
            // egui text fields cannot lay out bidi text, so right-to-left
            // titles are shown as display text until clicked.
            let resp = ui.add(
                egui::Label::new(
                    RichText::new(crate::ui::bidi::vis(&entry.data.title))
                        .text_style(theme::title_style()),
                )
                .sense(egui::Sense::click()),
            );
            if resp.on_hover_cursor(egui::CursorIcon::Text).clicked() {
                app.edit.title_editing = true;
                app.edit.focus_title = true;
            }
        } else {
            let resp = ui.add(
                crate::ui::field(&mut entry.data.title, false, 28.0)
                    .id(egui::Id::unique(("title_edit", id)))
                    .hint_text(crate::ui::hint(t!("collection.title_hint"), 20.0))
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY),
            );
            if std::mem::take(&mut app.edit.focus_title) {
                resp.request_focus();
            }
            if resp.changed() {
                dirty = true;
                app.edit.sync_folder.insert(id);
            }
            // Rename the folder only when the title field loses focus.
            if resp.lost_focus() {
                sync = true;
                app.edit.title_editing = false;
            }
        }
    });

    crate::ui::row_wrapped(ui, |ui| {
        let mut date = to_jiff(entry.data.date);
        let resp = ui.add(
            egui_extras::DatePickerButton::new(&mut date)
                .id_salt("collection_date")
                .calendar_week(false),
        );
        if resp.changed() && from_jiff(date) != entry.data.date {
            entry.data.date = from_jiff(date);
            dirty = true;
            sync = true;
        }
        ui.add_space(8.0);
        // Tags editor: chips with × and an input where Enter adds a tag.
        let mut remove = None;
        for (i, tag) in entry.data.tags.iter().enumerate() {
            egui::Frame::new()
                .fill(accent.gamma_multiply(0.2))
                .corner_radius(CornerRadius::same(255))
                .inner_margin(egui::Margin {
                    left: 10,
                    right: 4,
                    top: 2,
                    bottom: 2,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.label(
                        RichText::new(crate::ui::bidi::vis(&format!("#{tag}")))
                            .color(accent)
                            .size(13.0),
                    );
                    if ui
                        .add(egui::Button::new(RichText::new("×").size(13.0)).frame(false))
                        .on_hover_text(t!("collection.remove_tag"))
                        .clicked()
                    {
                        remove = Some(i);
                    }
                });
        }
        if let Some(i) = remove {
            entry.data.tags.remove(i);
            dirty = true;
        }
        let resp = ui.add(
            crate::ui::field(&mut app.edit.tag_input, false, 14.5)
                .hint_text(crate::ui::hint(t!("collection.add_tag"), 14.5))
                .desired_width(110.0),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let tag = app.edit.tag_input.trim().trim_start_matches('#').to_owned();
            if !tag.is_empty() && !entry.data.tags.contains(&tag) {
                entry.data.tags.push(tag);
                dirty = true;
            }
            app.edit.tag_input.clear();
            resp.request_focus();
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new(t!(
                "collection.count",
                n = entry.data.photos.len(),
                size = photovault::fsutil::human_bytes(entry.data.total_bytes())
            ))
            .color(theme::muted(ui)),
        );
        if entry.rename_pending {
            ui.colored_label(theme::AMBER, "⚠")
                .on_hover_text(t!("collection.rename_pending"));
        }
    });

    if dirty {
        app.actions.push(Action::MarkDirty(id));
    }
    if sync {
        app.actions.push(Action::SyncFolder(id));
    }
}

fn notes_card(app: &mut PhotoVaultApp, ui: &mut egui::Ui, id: Uuid) {
    let saving = app.edit.dirty.contains_key(&id);
    let error = app.edit.last_error.clone();
    let expanded = app.edit.notes_expanded;
    let Some(entry) = app.archive.as_mut().and_then(|a| a.get_mut(id)) else {
        return;
    };
    let mut changed = false;
    theme::card(ui).show(ui, |ui| {
        crate::ui::row(ui, |ui| {
            crate::ui::section(ui, t!("notes.title"));
            ui.with_layout(crate::ui::end_layout(), |ui| {
                if !app.edit.notes_editing
                    && photovault::bidi::has_rtl(&entry.data.notes)
                    && ui.small_button(t!("notes.edit")).clicked()
                {
                    app.edit.notes_editing = true;
                    app.edit.focus_notes = true;
                }
                if ui
                    .small_button(if expanded {
                        t!("notes.collapse")
                    } else {
                        t!("notes.expand")
                    })
                    .clicked()
                {
                    app.edit.notes_expanded = !expanded;
                }
                let (status, color) = match (&error, saving) {
                    (Some(_), _) => (t!("notes.error"), theme::DANGER),
                    (None, true) => (t!("notes.saving"), theme::muted(ui)),
                    (None, false) => (t!("notes.saved"), theme::SUCCESS),
                };
                let r = ui.label(RichText::new(status).small().color(color));
                if let Some(e) = &error {
                    r.on_hover_text(e);
                }
                let words = entry.data.notes.split_whitespace().count();
                let chars = entry.data.notes.chars().count();
                ui.label(
                    RichText::new(t!("notes.count", words = words, chars = chars))
                        .small()
                        .color(theme::muted(ui)),
                );
            });
        });
        ui.add_space(6.0);
        let max_h = if expanded { f32::INFINITY } else { 360.0 };
        let read_mode = !app.edit.notes_editing && photovault::bidi::has_rtl(&entry.data.notes);
        egui::ScrollArea::vertical()
            .id_salt(("notes", id))
            .max_height(max_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if read_mode {
                    // Bidi-correct reading view; click anywhere to edit.
                    let frame = egui::Frame::new()
                        .fill(ui.visuals().text_edit_bg_color())
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(10));
                    let resp = frame
                        .show(ui, |ui| {
                            ui.set_min_height(140.0);
                            crate::ui::bidi::paragraphs(
                                ui,
                                &entry.data.notes,
                                egui::FontId::proportional(15.0),
                                ui.visuals().text_color(),
                                photovault::i18n::is_rtl(),
                            )
                        })
                        .inner;
                    if resp.on_hover_cursor(egui::CursorIcon::Text).clicked() {
                        app.edit.notes_editing = true;
                        app.edit.focus_notes = true;
                    }
                } else {
                    let resp = ui.add(
                        crate::ui::field(&mut entry.data.notes, true, 15.0)
                            .id(egui::Id::unique(("notes_edit", id)))
                            .hint_text(crate::ui::hint(t!("notes.hint"), 14.5))
                            .desired_rows(8)
                            .desired_width(f32::INFINITY)
                            .lock_focus(true),
                    );
                    if std::mem::take(&mut app.edit.focus_notes) {
                        resp.request_focus();
                    }
                    if resp.changed() {
                        changed = true;
                    }
                    if resp.lost_focus() {
                        app.edit.notes_editing = false;
                    }
                }
            });
    });
    if changed {
        app.actions.push(Action::MarkDirty(id));
    }
}

fn actions_bar(app: &mut PhotoVaultApp, ui: &mut egui::Ui, id: Uuid, accent: Color32) {
    let Some(archive) = app.archive.as_ref() else {
        return;
    };
    let Some(entry) = archive.get(id) else { return };
    let dir = archive.root.join(&entry.rel_dir);
    let sel: Vec<String> = entry
        .data
        .photos
        .iter()
        .filter(|p| app.selection.contains(&p.file_name))
        .map(|p| p.file_name.clone())
        .collect();
    crate::ui::row_wrapped(ui, |ui| {
        if theme::primary_button(ui, accent, t!("actions.add_photos")).clicked() {
            app.actions.push(Action::AddPhotos(id));
        }
        if ui.button(t!("actions.open_folder")).clicked() {
            app.actions.push(Action::Open(dir.clone()));
        }
        if ui
            .add_enabled(sel.len() == 1, egui::Button::new(t!("actions.set_cover")))
            .clicked()
        {
            app.actions.push(Action::SetCover {
                id,
                file: sel[0].clone(),
            });
        }
        let remove_label = if sel.is_empty() {
            t!("actions.remove_selected").to_owned()
        } else {
            t!("actions.remove_n", n = sel.len())
        };
        if ui
            .add_enabled(!sel.is_empty(), egui::Button::new(remove_label))
            .clicked()
        {
            app.actions.push(Action::RemovePhotos {
                id,
                names: sel.clone(),
            });
        }
        if ui
            .add_enabled(
                !entry.data.photos.is_empty(),
                egui::Button::new(t!("actions.slideshow")),
            )
            .clicked()
        {
            app.actions.push(Action::Slideshow(id));
        }
        ui.menu_button(t!("actions.export"), |ui| {
            if ui.button(t!("actions.export_zip")).clicked() {
                app.actions.push(Action::ExportZip(id));
                ui.close();
            }
            if ui.button(t!("actions.export_html")).clicked() {
                app.actions.push(Action::ExportHtml(id));
                ui.close();
            }
        });
        if ui
            .button(RichText::new(t!("actions.delete_collection")).color(theme::DANGER))
            .clicked()
        {
            app.actions.push(Action::AskDeleteCollection(id));
        }
    });
    crate::ui::row(ui, |ui| {
        ui.label(RichText::new("🔍").color(theme::muted(ui)));
        let mut size = app.config.thumb_size;
        if ui
            .add(egui::Slider::new(&mut size, 90.0..=320.0).show_value(false))
            .on_hover_text(t!("actions.thumb_size"))
            .changed()
        {
            app.config.thumb_size = size;
            app.config_dirty = true;
        }
        if !sel.is_empty() {
            ui.label(RichText::new(t!("actions.selected", n = sel.len())).color(accent));
            if ui.small_button(t!("actions.clear_selection")).clicked() {
                app.selection.clear();
            }
        }
    });
}
