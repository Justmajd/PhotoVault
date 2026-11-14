//! Thumbnail painting and the responsive photo grid with multi-select.

use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Color32, CornerRadius, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle,
};
use photovault::archive::import::{MediaKind, extension_lower, media_kind};
use photovault::archive::model::Collection;
use photovault::t;
use uuid::Uuid;

use crate::app::{Action, PhotoVaultApp};
use crate::theme;
use crate::thumbs::{THUMB_PX, ThumbCache, ThumbKey, ThumbState};

/// UV rect that crops a texture to fill `target` (like CSS `object-fit: cover`).
pub fn cover_uv(tex: &TextureHandle, target: Rect) -> Rect {
    let [w, h] = tex.size();
    let (w, h) = (w as f32, h as f32);
    let ta = target.width() / target.height().max(1.0);
    let ia = w / h.max(1.0);
    if ia > ta {
        let frac = ta / ia;
        Rect::from_min_max(
            egui::pos2((1.0 - frac) / 2.0, 0.0),
            egui::pos2((1.0 + frac) / 2.0, 1.0),
        )
    } else {
        let frac = ia / ta;
        Rect::from_min_max(
            egui::pos2(0.0, (1.0 - frac) / 2.0),
            egui::pos2(1.0, (1.0 + frac) / 2.0),
        )
    }
}

/// Animated shimmer placeholder.
pub fn shimmer(ui: &egui::Ui, rect: Rect, radius: u8) {
    let p = ui.painter().with_clip_rect(rect);
    let base = if ui.visuals().dark_mode {
        Color32::from_rgb(0x26, 0x27, 0x2E)
    } else {
        Color32::from_rgb(0xE6, 0xE5, 0xE1)
    };
    p.rect_filled(rect, CornerRadius::same(radius), base);
    let t = ui.input(|i| i.time) as f32;
    let phase = (t * 0.8).fract();
    let band_w = rect.width() * 0.45;
    let x = rect.left() - band_w + phase * (rect.width() + band_w * 2.0);
    let steps = 8;
    for s in 0..steps {
        let f = s as f32 / steps as f32;
        let alpha = ((1.0 - (f * 2.0 - 1.0).abs()) * 22.0) as u8;
        let r = Rect::from_min_max(
            egui::pos2(x + f * band_w, rect.top()),
            egui::pos2(x + (f + 1.0 / steps as f32) * band_w, rect.bottom()),
        );
        p.rect_filled(r, CornerRadius::ZERO, Color32::from_white_alpha(alpha));
    }
    ui.ctx().request_repaint();
}

/// Generic tile for files that cannot be previewed (RAW, HEIC, video…).
pub fn generic_tile(ui: &egui::Ui, rect: Rect, file_name: &str, radius: u8, accent: Color32) {
    let p = ui.painter();
    let fill = if ui.visuals().dark_mode {
        Color32::from_rgb(0x2A, 0x2B, 0x33)
    } else {
        Color32::from_rgb(0xE9, 0xE8, 0xE4)
    };
    p.rect_filled(rect, CornerRadius::same(radius), fill);
    let ext = extension_lower(Path::new(file_name)).to_uppercase();
    let is_video = matches!(ext.as_str(), "MP4" | "MOV" | "M4V" | "3GP");
    let icon = if is_video { "▶" } else { "🖼" };
    let size = (rect.height() * 0.22).clamp(12.0, 40.0);
    p.text(
        rect.center() - egui::vec2(0.0, size * 0.4),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(size),
        accent,
    );
    let badge = egui::Rect::from_center_size(
        rect.center() + egui::vec2(0.0, size * 0.7),
        egui::vec2(ext.len() as f32 * 8.0 + 14.0, 18.0),
    );
    if rect.height() > 60.0 {
        p.rect_filled(badge, CornerRadius::same(9), accent.gamma_multiply(0.85));
        p.text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            &ext,
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );
    }
}

/// Paints one photo into `rect` (cover-cropped), with shimmer while loading.
pub fn paint_thumb(
    ui: &egui::Ui,
    thumbs: &mut ThumbCache,
    path: &Path,
    sha: &str,
    rect: Rect,
    radius: u8,
    accent: Color32,
) {
    paint_thumb_corners(
        ui,
        thumbs,
        path,
        sha,
        rect,
        CornerRadius::same(radius),
        accent,
    );
}

/// Like [`paint_thumb`] with individual corner radii.
pub fn paint_thumb_corners(
    ui: &egui::Ui,
    thumbs: &mut ThumbCache,
    path: &Path,
    sha: &str,
    rect: Rect,
    corners: CornerRadius,
    accent: Color32,
) {
    let radius = corners.nw.max(corners.ne).max(corners.sw).max(corners.se);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if media_kind(path) != MediaKind::Previewable {
        generic_tile(ui, rect, &name, radius, accent);
        return;
    }
    if !ui.is_rect_visible(rect) {
        return;
    }
    let key = ThumbKey {
        path: path.to_path_buf(),
        max_px: THUMB_PX,
    };
    match thumbs.get(&key, sha) {
        ThumbState::Ready(tex, age) => {
            // Fade freshly decoded thumbnails in over the shimmer.
            let fade = crate::anim::ease_out(age / 0.25);
            if fade < 1.0 {
                shimmer(ui, rect, radius);
            }
            let tint = Color32::WHITE.gamma_multiply(fade);
            let uv = cover_uv(tex, rect);
            let mut mesh = egui::Mesh::with_texture(tex.id());
            mesh.add_rect_with_uv(rect, uv, tint);
            if radius == 0 {
                ui.painter().add(mesh);
            } else {
                ui.painter().add(
                    egui::epaint::RectShape::filled(rect, corners, Color32::WHITE)
                        .with_texture(tex.id(), uv),
                );
            }
        }
        ThumbState::Loading => shimmer(ui, rect, radius),
        ThumbState::Failed => generic_tile(ui, rect, &name, radius, accent),
    }
}

/// Cover image for a collection: the cover photo, or a 2×2 mosaic of the first
/// photos when no cover is set.
pub fn paint_cover(
    ui: &egui::Ui,
    thumbs: &mut ThumbCache,
    dir: &Path,
    c: &Collection,
    rect: Rect,
    radius: u8,
    accent: Color32,
) {
    if let Some(cover) = c
        .cover_photo
        .as_deref()
        .and_then(|f| c.photos.iter().find(|p| p.file_name == f))
    {
        paint_thumb(
            ui,
            thumbs,
            &dir.join(&cover.file_name),
            &cover.sha256,
            rect,
            radius,
            accent,
        );
        return;
    }
    let previewable: Vec<_> = c
        .photos
        .iter()
        .filter(|p| media_kind(Path::new(&p.file_name)) == MediaKind::Previewable)
        .take(4)
        .collect();
    match previewable.len() {
        0 => {
            if let Some(p) = c.photos.first() {
                generic_tile(ui, rect, &p.file_name, radius, accent);
            } else {
                let fill = accent.gamma_multiply(0.18);
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(radius), fill);
                theme::paint_logo(
                    ui.painter(),
                    rect.shrink(rect.height() * 0.28),
                    accent.gamma_multiply(0.6),
                );
            }
        }
        1..=3 => paint_thumb(
            ui,
            thumbs,
            &dir.join(&previewable[0].file_name),
            &previewable[0].sha256,
            rect,
            radius,
            accent,
        ),
        _ => {
            let gap = 1.5;
            let half = (rect.size() - egui::vec2(gap, gap)) / 2.0;
            for (i, p) in previewable.iter().enumerate() {
                let x = if i % 2 == 0 {
                    rect.left()
                } else {
                    rect.left() + half.x + gap
                };
                let y = if i < 2 {
                    rect.top()
                } else {
                    rect.top() + half.y + gap
                };
                let r = Rect::from_min_size(egui::pos2(x, y), half);
                // Only the outer corner of each quadrant is rounded.
                let corners = match i {
                    0 => CornerRadius {
                        nw: radius,
                        ..CornerRadius::ZERO
                    },
                    1 => CornerRadius {
                        ne: radius,
                        ..CornerRadius::ZERO
                    },
                    2 => CornerRadius {
                        sw: radius,
                        ..CornerRadius::ZERO
                    },
                    _ => CornerRadius {
                        se: radius,
                        ..CornerRadius::ZERO
                    },
                };
                paint_thumb_corners(
                    ui,
                    thumbs,
                    &dir.join(&p.file_name),
                    &p.sha256,
                    r,
                    corners,
                    accent,
                );
            }
        }
    }
}

/// The photo grid for a collection.
pub fn show(app: &mut PhotoVaultApp, ui: &mut egui::Ui, id: Uuid) {
    let accent = app.accent();
    let Some(archive) = app.archive.as_ref() else {
        return;
    };
    let Some(entry) = archive.get(id) else { return };
    let dir: PathBuf = archive.root.join(&entry.rel_dir);
    let size = app.config.thumb_size;
    let gap = 10.0;
    let width = ui.available_width();
    let cols = (((width + gap) / (size + gap)).floor() as usize).max(1);
    let cell = ((width - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    let n = entry.data.photos.len();
    let (cmd, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
    let other_collections: Vec<(Uuid, String)> = archive
        .collections
        .iter()
        .filter(|c| c.data.id != id)
        .map(|c| (c.data.id, crate::ui::bidi::vis(&c.data.title)))
        .collect();
    let cover = entry.data.cover_photo.clone();

    let mut clicked: Option<usize> = None;
    let rows = n.div_ceil(cols);
    for row in 0..rows {
        let (row_rect, _) = ui.allocate_exact_size(egui::vec2(width, cell), Sense::hover());
        if !ui.is_rect_visible(row_rect) {
            ui.add_space(gap - ui.spacing().item_spacing.y);
            continue;
        }
        for col in 0..cols {
            let i = row * cols + col;
            if i >= n {
                break;
            }
            let photo = &entry.data.photos[i];
            // Columns run right-to-left in the Arabic UI.
            let x = if crate::ui::rtl() {
                row_rect.right() - cell - col as f32 * (cell + gap)
            } else {
                row_rect.left() + col as f32 * (cell + gap)
            };
            let slot = Rect::from_min_size(egui::pos2(x, row_rect.top()), egui::vec2(cell, cell));
            let resp = ui.interact(slot, ui.scope_id().with(("tile", id, i)), Sense::click());
            // Hover: the tile grows slightly; selection ring fades in.
            let anim_t = if app.config.animations { 0.14 } else { 0.0 };
            let hover = ui.ctx().animate_bool_with_time(
                egui::Id::unique(("tile_hover", id, i)),
                resp.hovered(),
                anim_t,
            );
            let sel_anim = ui.ctx().animate_bool_with_time(
                egui::Id::unique(("tile_sel", id, i)),
                app.selection.contains(&photo.file_name),
                anim_t,
            );
            let rect = slot.expand(hover * 3.0);
            if hover > 0.0 {
                let shadow = egui::epaint::Shadow {
                    offset: [0, 4],
                    blur: (12.0 * hover) as u8,
                    spread: 0,
                    color: Color32::from_black_alpha((60.0 * hover) as u8),
                };
                ui.painter()
                    .add(shadow.as_shape(rect, CornerRadius::same(10)));
            }
            let path = dir.join(&photo.file_name);
            let missing = entry.missing.contains(&photo.file_name);
            if missing {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(10),
                    theme::AMBER.gamma_multiply(0.15),
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t!("grid.missing"),
                    egui::FontId::proportional(13.0),
                    theme::AMBER,
                );
            } else {
                paint_thumb(ui, &mut app.thumbs, &path, &photo.sha256, rect, 10, accent);
            }
            let selected = app.selection.contains(&photo.file_name);
            if hover > 0.0 && !selected {
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(10),
                    Stroke::new(2.0, accent.gamma_multiply(0.6 * hover)),
                    StrokeKind::Inside,
                );
            }
            if sel_anim > 0.0 {
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(10),
                    Stroke::new(3.0, accent.gamma_multiply(sel_anim)),
                    StrokeKind::Inside,
                );
            }
            if selected {
                let c = rect.right_top() + egui::vec2(-16.0, 16.0);
                ui.painter()
                    .circle_filled(c, 10.0 * crate::anim::ease_out_back(sel_anim), accent);
                ui.painter().text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    "✓",
                    egui::FontId::proportional(13.0),
                    Color32::WHITE,
                );
            }
            let mut badge_x = rect.left() + 8.0;
            if photo.favorite {
                ui.painter().text(
                    egui::pos2(badge_x, rect.top() + 8.0),
                    egui::Align2::LEFT_TOP,
                    "★",
                    egui::FontId::proportional(16.0),
                    theme::AMBER,
                );
                badge_x += 18.0;
            }
            if cover.as_deref() == Some(photo.file_name.as_str()) {
                ui.painter().text(
                    egui::pos2(badge_x, rect.top() + 8.0),
                    egui::Align2::LEFT_TOP,
                    t!("grid.cover"),
                    egui::FontId::proportional(11.0),
                    Color32::WHITE,
                );
            }
            if !photo.caption.is_empty() && cell > 110.0 {
                let strip =
                    Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 24.0), rect.max);
                ui.painter().rect_filled(
                    strip,
                    CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: 10,
                        se: 10,
                    },
                    Color32::from_black_alpha(140),
                );
                let text =
                    crate::ui::bidi::vis(&photo.caption.chars().take(40).collect::<String>());
                let rtl_caption = photovault::bidi::is_rtl_paragraph(&photo.caption);
                let (pos, align) = if rtl_caption {
                    (
                        strip.right_center() - egui::vec2(8.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                    )
                } else {
                    (
                        strip.left_center() + egui::vec2(8.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                    )
                };
                ui.painter().with_clip_rect(strip).text(
                    pos,
                    align,
                    text,
                    egui::FontId::proportional(12.0),
                    Color32::WHITE,
                );
            }
            let resp = resp.on_hover_ui(|ui| {
                ui.label(RichText::new(&photo.file_name).strong());
                if let Some(t) = photo.taken_at {
                    ui.label(t!("grid.taken", date = t.format("%Y-%m-%d %H:%M")));
                }
                if missing {
                    ui.colored_label(theme::AMBER, t!("grid.missing_tip"));
                }
            });
            if resp.clicked() {
                clicked = Some(i);
            }
            let file_name = photo.file_name.clone();
            resp.context_menu(|ui| {
                if !app.selection.contains(&file_name) {
                    app.selection.clear();
                    app.selection.insert(file_name.clone());
                }
                if ui.button(t!("grid.open")).clicked() {
                    app.actions.push(Action::OpenViewer { id, index: i });
                    ui.close();
                }
                if ui.button(t!("actions.set_cover")).clicked() {
                    app.actions.push(Action::SetCover {
                        id,
                        file: file_name.clone(),
                    });
                    ui.close();
                }
                if ui.button(t!("viewer.open_system")).clicked() {
                    app.actions.push(Action::Open(path.clone()));
                    ui.close();
                }
                if ui.button(t!("viewer.show_in_folder")).clicked() {
                    app.actions.push(Action::Reveal(path.clone()));
                    ui.close();
                }
                ui.separator();
                ui.menu_button(t!("grid.reorder"), |ui| {
                    if ui.button(t!("grid.move_first")).clicked() {
                        app.actions.push(Action::Reorder {
                            id,
                            file: file_name.clone(),
                            to: 0,
                        });
                        ui.close();
                    }
                    if ui
                        .add_enabled(i > 0, egui::Button::new(t!("grid.move_earlier")))
                        .clicked()
                    {
                        app.actions.push(Action::Reorder {
                            id,
                            file: file_name.clone(),
                            to: i.saturating_sub(1),
                        });
                        ui.close();
                    }
                    if ui
                        .add_enabled(i + 1 < n, egui::Button::new(t!("grid.move_later")))
                        .clicked()
                    {
                        app.actions.push(Action::Reorder {
                            id,
                            file: file_name.clone(),
                            to: i + 1,
                        });
                        ui.close();
                    }
                    if ui.button(t!("grid.move_last")).clicked() {
                        app.actions.push(Action::Reorder {
                            id,
                            file: file_name.clone(),
                            to: n,
                        });
                        ui.close();
                    }
                });
                if !other_collections.is_empty() {
                    ui.menu_button(t!("viewer.move_to"), |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(300.0)
                            .show(ui, |ui| {
                                for (cid, title) in &other_collections {
                                    if ui.button(title).clicked() {
                                        let names: Vec<String> =
                                            app.selection.iter().cloned().collect();
                                        app.actions.push(Action::MovePhotos {
                                            from: id,
                                            to: *cid,
                                            names,
                                        });
                                        ui.close();
                                    }
                                }
                            });
                    });
                }
                ui.separator();
                let count = app.selection.len().max(1);
                if ui
                    .button(RichText::new(t!("actions.remove_n", n = count)).color(theme::DANGER))
                    .clicked()
                {
                    let names: Vec<String> = app.selection.iter().cloned().collect();
                    app.actions.push(Action::RemovePhotos { id, names });
                    ui.close();
                }
            });
        }
        ui.add_space(gap - ui.spacing().item_spacing.y);
    }

    if let Some(i) = clicked {
        let name = entry.data.photos[i].file_name.clone();
        if cmd {
            if !app.selection.remove(&name) {
                app.selection.insert(name);
            }
            app.anchor = Some(i);
        } else if shift {
            let a = app.anchor.unwrap_or(0).min(n.saturating_sub(1));
            let (lo, hi) = if a <= i { (a, i) } else { (i, a) };
            for p in &entry.data.photos[lo..=hi] {
                app.selection.insert(p.file_name.clone());
            }
        } else {
            app.anchor = Some(i);
            app.selection.clear();
            app.actions.push(Action::OpenViewer { id, index: i });
        }
    }
}
