//! Full-window photo viewer (lightbox) with zoom/pan and an info panel.

use std::path::Path;

use eframe::egui::{self, Color32, Key, Rect, RichText, Sense};
use photovault::archive::import::{MediaKind, media_kind};
use photovault::fsutil::human_bytes;
use photovault::t;

use crate::app::{Action, PhotoVaultApp};
use crate::theme;
use crate::thumbs::{LARGE_PX, THUMB_PX, ThumbKey, ThumbState};
use crate::ui::photo_grid;

pub fn show(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    let accent = app.accent();
    let Some(v) = app.viewer.as_ref() else { return };
    let (id, index) = (v.id, v.index);
    let Some(archive) = app.archive.as_ref() else {
        app.viewer = None;
        return;
    };
    let Some(entry) = archive.get(id) else {
        app.viewer = None;
        return;
    };
    let n = entry.data.photos.len();
    if n == 0 {
        app.viewer = None;
        return;
    }
    let index = index.min(n - 1);
    let dir = archive.root.join(&entry.rel_dir);
    let photo = entry.data.photos[index].clone();
    let path = dir.join(&photo.file_name);
    let is_cover = entry.data.cover_photo.as_deref() == Some(photo.file_name.as_str());
    let others: Vec<(uuid::Uuid, String)> = archive
        .collections
        .iter()
        .filter(|c| c.data.id != id)
        .map(|c| (c.data.id, crate::ui::bidi::vis(&c.data.title)))
        .collect();
    let neighbours: Vec<(std::path::PathBuf, String)> =
        [index.checked_sub(1), (index + 1 < n).then_some(index + 1)]
            .into_iter()
            .flatten()
            .map(|i| {
                (
                    dir.join(&entry.data.photos[i].file_name),
                    entry.data.photos[i].sha256.clone(),
                )
            })
            .collect();

    // Keyboard navigation (not while typing a caption).
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (left, right, esc, f_key) = ui.input(|i| {
        (
            i.key_pressed(Key::ArrowLeft),
            i.key_pressed(Key::ArrowRight),
            i.key_pressed(Key::Escape),
            i.key_pressed(Key::F),
        )
    });
    let mut new_index = index;
    let (left, right) = if crate::ui::rtl() {
        (right, left)
    } else {
        (left, right)
    };
    if !typing {
        if left && index > 0 {
            new_index = index - 1;
        }
        if right && index + 1 < n {
            new_index = index + 1;
        }
        if f_key {
            let fs = !app.fullscreen;
            app.set_fullscreen(ui.ctx(), fs);
        }
    }
    if esc {
        if typing {
            ui.ctx().memory_mut(|m| m.stop_text_input());
        } else {
            if app.fullscreen {
                app.set_fullscreen(ui.ctx(), false);
            }
            app.viewer = None;
            return;
        }
    }

    // Preload neighbours for instant navigation.
    for (p, sha) in &neighbours {
        if media_kind(p) == MediaKind::Previewable {
            app.thumbs.prefetch(
                &ThumbKey {
                    path: p.clone(),
                    max_px: LARGE_PX,
                },
                sha,
            );
        }
    }

    let bg = Color32::from_rgb(0x0B, 0x0B, 0x0D);
    let mut caption_changed = false;
    let mut favorite_toggle = false;

    let info_panel = if crate::ui::rtl() {
        egui::Panel::left("viewer_info")
    } else {
        egui::Panel::right("viewer_info")
    };
    info_panel
        .resizable(false)
        .exact_size(330.0)
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().panel_fill)
                .inner_margin(egui::Margin::same(18)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.with_layout(crate::ui::column_layout(), |ui| {
                    crate::ui::row(ui, |ui| {
                        ui.label(
                            RichText::new(t!("viewer.counter", i = index + 1, n = n))
                                .color(theme::muted(ui)),
                        );
                        ui.with_layout(crate::ui::end_layout(), |ui| {
                            if ui.button("×").on_hover_text(t!("viewer.close")).clicked() {
                                app.viewer = None;
                            }
                        });
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new(t!("viewer.caption")).strong());
                    if let Some(c) = app
                        .archive
                        .as_mut()
                        .and_then(|a| a.get_mut(id))
                        .and_then(|c| c.data.photos.get_mut(index))
                    {
                        let r = ui.add(
                            crate::ui::field(&mut c.caption, true, 15.0)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY)
                                .hint_text(crate::ui::hint(t!("viewer.caption_hint"), 14.5)),
                        );
                        caption_changed = r.changed();
                    }
                    ui.add_space(10.0);
                    let row = |ui: &mut egui::Ui, k: &str, v: String| {
                        crate::ui::row_wrapped(ui, |ui| {
                            ui.label(RichText::new(k).small().color(theme::muted(ui)));
                            ui.label(crate::ui::bidi::vis(&v));
                        });
                    };
                    row(ui, t!("viewer.file"), photo.file_name.clone());
                    if photo.original_name != photo.file_name && !photo.original_name.is_empty() {
                        row(ui, t!("viewer.original"), photo.original_name.clone());
                    }
                    if let Some(t) = photo.taken_at {
                        row(
                            ui,
                            t!("viewer.taken"),
                            t.format("%Y-%m-%d %H:%M:%S").to_string(),
                        );
                    }
                    if let Some(c) = &photo.camera {
                        row(ui, t!("viewer.camera"), c.clone());
                    }
                    if let (Some(w), Some(h)) = (photo.width, photo.height) {
                        row(ui, t!("viewer.dimensions"), format!("{w} × {h}"));
                    }
                    row(ui, t!("viewer.size"), human_bytes(photo.size_bytes));
                    crate::ui::row(ui, |ui| {
                        ui.label(RichText::new("SHA-256").small().color(theme::muted(ui)));
                        let short = if photo.sha256.len() > 16 {
                            format!("{}…", &photo.sha256[..16])
                        } else {
                            photo.sha256.clone()
                        };
                        ui.label(RichText::new(short).monospace())
                            .on_hover_text(&photo.sha256);
                        if ui.small_button(t!("viewer.copy")).clicked() {
                            ui.ctx().copy_text(photo.sha256.clone());
                            app.actions.push(Action::Toast(
                                t!("toast.copied").to_owned(),
                                crate::ui::toasts::ToastKind::Info,
                            ));
                        }
                    });
                    if let Some(g) = photo.gps {
                        crate::ui::row_wrapped(ui, |ui| {
                            ui.label(
                                RichText::new(t!("viewer.gps"))
                                    .small()
                                    .color(theme::muted(ui)),
                            );
                            ui.label(format!("{:.5}, {:.5}", g.lat, g.lon));
                            if ui.link(t!("viewer.osm")).clicked() {
                                app.actions.push(Action::OpenUrl(g.osm_url()));
                            }
                        });
                    }
                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(6.0);
                    let fav = if photo.favorite {
                        t!("viewer.unfavorite")
                    } else {
                        t!("viewer.favorite")
                    };
                    if ui.button(fav).clicked() {
                        favorite_toggle = true;
                    }
                    if ui
                        .add_enabled(!is_cover, egui::Button::new(t!("actions.set_cover")))
                        .clicked()
                    {
                        app.actions.push(Action::SetCover {
                            id,
                            file: photo.file_name.clone(),
                        });
                    }
                    if ui.button(t!("viewer.open_system")).clicked() {
                        app.actions.push(Action::Open(path.clone()));
                    }
                    if ui.button(t!("viewer.show_in_folder")).clicked() {
                        app.actions.push(Action::Reveal(path.clone()));
                    }
                    if ui
                        .add_enabled(!app.fullscreen, egui::Button::new(t!("viewer.fullscreen")))
                        .clicked()
                    {
                        app.set_fullscreen(ui.ctx(), true);
                    }
                    ui.add_space(6.0);
                    if !others.is_empty() {
                        ui.label(
                            RichText::new(t!("viewer.move_to"))
                                .small()
                                .color(theme::muted(ui)),
                        );
                        let v = app.viewer.as_mut();
                        if let Some(v) = v {
                            let current = v
                                .move_target
                                .and_then(|t| others.iter().find(|(i, _)| *i == t))
                                .map(|(_, s)| s.clone());
                            crate::ui::row(ui, |ui| {
                                egui::ComboBox::from_id_salt("move_target")
                                    .width(200.0)
                                    .selected_text(
                                        current.unwrap_or_else(|| t!("viewer.choose").to_owned()),
                                    )
                                    .show_ui(ui, |ui| {
                                        for (oid, title) in &others {
                                            ui.selectable_value(
                                                &mut v.move_target,
                                                Some(*oid),
                                                title,
                                            );
                                        }
                                    });
                                if ui
                                    .add_enabled(
                                        v.move_target.is_some(),
                                        egui::Button::new(t!("viewer.move")),
                                    )
                                    .clicked()
                                    && let Some(to) = v.move_target
                                {
                                    app.actions.push(Action::MovePhotos {
                                        from: id,
                                        to,
                                        names: vec![photo.file_name.clone()],
                                    });
                                }
                            });
                        }
                    }
                    ui.add_space(10.0);
                    if ui
                        .button(RichText::new(t!("viewer.remove")).color(theme::DANGER))
                        .clicked()
                    {
                        app.actions.push(Action::RemovePhotos {
                            id,
                            names: vec![photo.file_name.clone()],
                        });
                    }
                    ui.add_space(14.0);
                    let c = theme::muted(ui);
                    crate::ui::bidi::para(ui, t!("viewer.keys"), 12.0, Some(c));
                })
            });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(bg))
        .show(ui, |ui| {
            let area = ui.max_rect();
            let resp = ui.interact(
                area,
                ui.scope_id().with("viewer_canvas"),
                Sense::click_and_drag(),
            );
            let Some(v) = app.viewer.as_mut() else { return };
            if media_kind(&path) != MediaKind::Previewable {
                let r = Rect::from_center_size(area.center(), egui::vec2(260.0, 260.0));
                photo_grid::generic_tile(ui, r, &photo.file_name, 18, accent);
                ui.painter().text(
                    r.center_bottom() + egui::vec2(0.0, 24.0),
                    egui::Align2::CENTER_TOP,
                    t!("viewer.no_preview"),
                    egui::FontId::proportional(14.0),
                    Color32::GRAY,
                );
            } else {
                let large = ThumbKey {
                    path: path.clone(),
                    max_px: LARGE_PX,
                };
                let small = ThumbKey {
                    path: path.clone(),
                    max_px: THUMB_PX,
                };
                let tex = match app.thumbs.get(&large, &photo.sha256) {
                    ThumbState::Ready(t, _) => Some(t.clone()),
                    ThumbState::Failed => None,
                    ThumbState::Loading => match app.thumbs.get(&small, &photo.sha256) {
                        ThumbState::Ready(t, _) => Some(t.clone()),
                        _ => None,
                    },
                };
                match tex {
                    Some(tex) => {
                        // Zoom with the wheel around the pointer, pan by dragging.
                        let scroll =
                            ui.input(|i| i.smooth_scroll_delta.y + (i.zoom_delta() - 1.0) * 200.0);
                        let [tw, th] = tex.size();
                        let fit = (area.width() / tw as f32)
                            .min(area.height() / th as f32)
                            .min(if tw as u32 >= LARGE_PX || th as u32 >= LARGE_PX {
                                1.0
                            } else {
                                4.0
                            });
                        let fit = if fit.is_finite() && fit > 0.0 {
                            fit
                        } else {
                            1.0
                        };
                        if resp.hovered() && scroll.abs() > 0.0 {
                            let old = v.zoom;
                            v.zoom = (v.zoom * (1.0 + scroll * 0.0025)).clamp(1.0, 12.0);
                            if let Some(pos) = resp.hover_pos() {
                                let rel = pos - area.center() - v.pan;
                                v.pan -= rel * (v.zoom / old - 1.0);
                            }
                        }
                        if resp.dragged() {
                            v.pan += resp.drag_delta();
                        }
                        if resp.double_clicked() {
                            v.zoom = if v.zoom > 1.01 { 1.0 } else { 2.5 };
                            v.pan = egui::Vec2::ZERO;
                        }
                        if v.zoom <= 1.0 {
                            v.pan = egui::Vec2::ZERO;
                        }
                        // Smooth zoom, plus a fade/scale-in whenever the photo changes.
                        let anim_t = if app.config.animations { 0.12 } else { 0.0 };
                        let zoom = ui.ctx().animate_value_with_time(
                            egui::Id::unique(("viewer_zoom", id)),
                            v.zoom,
                            anim_t,
                        );
                        let appear = crate::anim::ease_out(crate::anim::progress(
                            ui.ctx(),
                            v.shown_at,
                            0.24,
                            app.config.animations,
                        ));
                        let size =
                            egui::vec2(tw as f32, th as f32) * fit * zoom * (0.97 + 0.03 * appear);
                        let rect = Rect::from_center_size(area.center() + v.pan, size);
                        ui.painter().with_clip_rect(area).image(
                            tex.id(),
                            rect,
                            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE.gamma_multiply(appear),
                        );
                    }
                    None => {
                        ui.painter().text(
                            area.center(),
                            egui::Align2::CENTER_CENTER,
                            t!("viewer.loading"),
                            egui::FontId::proportional(15.0),
                            Color32::GRAY,
                        );
                        ui.ctx().request_repaint();
                    }
                }
            }
            // Prev/next arrows.
            let arrow = |ui: &mut egui::Ui, rect: Rect, text: &str, enabled: bool| -> bool {
                if !enabled {
                    return false;
                }
                let r = ui.interact(rect, ui.scope_id().with(text), Sense::click());
                let alpha = if r.hovered() { 200 } else { 90 };
                ui.painter()
                    .circle_filled(rect.center(), 22.0, Color32::from_black_alpha(alpha));
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(22.0),
                    Color32::WHITE,
                );
                r.clicked()
            };
            let l = Rect::from_center_size(
                egui::pos2(area.left() + 40.0, area.center().y),
                egui::vec2(48.0, 48.0),
            );
            let r = Rect::from_center_size(
                egui::pos2(area.right() - 40.0, area.center().y),
                egui::vec2(48.0, 48.0),
            );
            // In the Arabic UI "next" is on the left.
            let (prev_rect, next_rect) = if crate::ui::rtl() { (r, l) } else { (l, r) };
            let (prev_glyph, next_glyph) = if crate::ui::rtl() {
                ("›", "‹")
            } else {
                ("‹", "›")
            };
            if arrow(ui, prev_rect, prev_glyph, index > 0) {
                new_index = index - 1;
            }
            if arrow(ui, next_rect, next_glyph, index + 1 < n) {
                new_index = index + 1;
            }
            if photo.favorite {
                ui.painter().text(
                    area.left_top() + egui::vec2(20.0, 16.0),
                    egui::Align2::LEFT_TOP,
                    "★",
                    egui::FontId::proportional(22.0),
                    theme::AMBER,
                );
            }
        });

    if favorite_toggle
        && let Some(p) = app
            .archive
            .as_mut()
            .and_then(|a| a.get_mut(id))
            .and_then(|c| c.data.photos.get_mut(index))
    {
        p.favorite = !p.favorite;
        app.actions.push(Action::MarkDirty(id));
    }
    if caption_changed {
        app.actions.push(Action::MarkDirty(id));
    }
    if let Some(v) = app.viewer.as_mut()
        && new_index != v.index
    {
        v.index = new_index;
        v.zoom = 1.0;
        v.pan = egui::Vec2::ZERO;
        v.shown_at = std::time::Instant::now();
    }
    let _ = Path::new("");
}
