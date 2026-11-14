//! Full-screen slideshow with captions and interval control.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Key, Rect, RichText};
use photovault::archive::import::{MediaKind, media_kind};
use photovault::t;

use crate::app::PhotoVaultApp;
use crate::thumbs::{LARGE_PX, ThumbKey, ThumbState};
use crate::ui::photo_grid;

pub fn show(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    let accent = app.accent();
    let Some(s) = app.slideshow.as_ref() else {
        return;
    };
    let (id, index, playing) = (s.id, s.index, s.playing);
    let Some(entry) = app.archive.as_ref().and_then(|a| a.get(id)) else {
        app.slideshow = None;
        return;
    };
    let dir = app
        .archive
        .as_ref()
        .map(|a| a.root.join(&entry.rel_dir))
        .unwrap_or_default();
    let n = entry.data.photos.len();
    if n == 0 {
        app.slideshow = None;
        return;
    }
    let index = index % n;
    let photo = entry.data.photos[index].clone();
    let next = entry.data.photos[(index + 1) % n].clone();
    let title = crate::ui::bidi::vis(&entry.data.title);
    let path = dir.join(&photo.file_name);
    let next_path = dir.join(&next.file_name);

    let (esc, left, right, space) = ui.input(|i| {
        (
            i.key_pressed(Key::Escape),
            i.key_pressed(Key::ArrowLeft),
            i.key_pressed(Key::ArrowRight),
            i.key_pressed(Key::Space),
        )
    });
    if esc {
        app.slideshow = None;
        app.set_fullscreen(ui.ctx(), false);
        return;
    }
    let interval = Duration::from_secs_f32(app.config.slideshow_secs.max(1.0));
    let mut new_index = index;
    let mut toggle = space;
    if right {
        new_index = (index + 1) % n;
    }
    if left {
        new_index = (index + n - 1) % n;
    }
    if playing {
        let elapsed = app
            .slideshow
            .as_ref()
            .map(|s| s.last_switch.elapsed())
            .unwrap_or_default();
        if elapsed >= interval {
            new_index = (index + 1) % n;
        } else {
            ui.ctx().request_repaint_after(interval - elapsed);
        }
    }
    if media_kind(&next_path) == MediaKind::Previewable {
        app.thumbs.prefetch(
            &ThumbKey {
                path: next_path,
                max_px: LARGE_PX,
            },
            &next.sha256,
        );
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(Color32::BLACK))
        .show(ui, |ui| {
            let area = ui.max_rect();
            if media_kind(&path) == MediaKind::Previewable {
                match app.thumbs.get(
                    &ThumbKey {
                        path: path.clone(),
                        max_px: LARGE_PX,
                    },
                    &photo.sha256,
                ) {
                    ThumbState::Ready(tex, _) => {
                        let [w, h] = tex.size();
                        let fit = (area.width() / w as f32).min(area.height() / h as f32);
                        let rect = Rect::from_center_size(
                            area.center(),
                            egui::vec2(w as f32, h as f32) * fit,
                        );
                        ui.painter().image(
                            tex.id(),
                            rect,
                            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    ThumbState::Loading => {
                        ui.painter().text(
                            area.center(),
                            egui::Align2::CENTER_CENTER,
                            t!("viewer.loading"),
                            egui::FontId::proportional(16.0),
                            Color32::GRAY,
                        );
                    }
                    ThumbState::Failed => photo_grid::generic_tile(
                        ui,
                        Rect::from_center_size(area.center(), egui::vec2(240.0, 240.0)),
                        &photo.file_name,
                        18,
                        accent,
                    ),
                }
            } else {
                photo_grid::generic_tile(
                    ui,
                    Rect::from_center_size(area.center(), egui::vec2(240.0, 240.0)),
                    &photo.file_name,
                    18,
                    accent,
                );
            }
            // Caption overlay.
            let caption = if photo.caption.trim().is_empty() {
                title.clone()
            } else {
                crate::ui::bidi::vis(&photo.caption)
            };
            let galley = ui.painter().layout(
                caption,
                egui::FontId::proportional(20.0),
                Color32::WHITE,
                area.width() * 0.7,
            );
            let cap_rect = Rect::from_center_size(
                egui::pos2(area.center().x, area.bottom() - 70.0),
                galley.size() + egui::vec2(32.0, 18.0),
            );
            ui.painter().rect_filled(
                cap_rect,
                egui::CornerRadius::same(12),
                Color32::from_black_alpha(150),
            );
            ui.painter()
                .galley(cap_rect.min + egui::vec2(16.0, 9.0), galley, Color32::WHITE);

            // Controls appear while the pointer moves.
            let moving = ui.input(|i| i.pointer.time_since_last_movement() < 2.5);
            if moving {
                let bar = Rect::from_center_size(
                    egui::pos2(area.center().x, area.top() + 34.0),
                    egui::vec2(560.0, 44.0),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
                    egui::Frame::new()
                        .fill(Color32::from_black_alpha(170))
                        .corner_radius(egui::CornerRadius::same(22))
                        .inner_margin(egui::Margin::symmetric(14, 6))
                        .show(ui, |ui| {
                            crate::ui::row(ui, |ui| {
                                ui.visuals_mut().override_text_color = Some(Color32::WHITE);
                                if ui.button("‹").clicked() {
                                    new_index = (index + n - 1) % n;
                                }
                                if ui.button(if playing { "⏸" } else { "▶" }).clicked() {
                                    toggle = true;
                                }
                                if ui.button("›").clicked() {
                                    new_index = (index + 1) % n;
                                }
                                ui.label(format!("{} / {}", index + 1, n));
                                ui.add(
                                    egui::Slider::new(&mut app.config.slideshow_secs, 1.0..=20.0)
                                        .suffix(" s")
                                        .text(t!("slideshow.interval")),
                                );
                                if ui.button(RichText::new(t!("slideshow.exit"))).clicked() {
                                    app.actions.push(crate::app::Action::ShowView(
                                        crate::app::View::Collection(id),
                                    ));
                                    app.slideshow = None;
                                }
                            });
                        });
                });
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        });

    if app.slideshow.is_none() {
        app.set_fullscreen(ui.ctx(), false);
        app.config_dirty = true;
        return;
    }
    if let Some(s) = app.slideshow.as_mut() {
        if toggle {
            s.playing = !s.playing;
            s.last_switch = Instant::now();
        }
        if new_index != s.index {
            s.index = new_index;
            s.last_switch = Instant::now();
        }
    }
}
