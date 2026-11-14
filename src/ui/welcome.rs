//! First-run / archive selection screen.

use eframe::egui::{self, RichText};
use photovault::t;

use crate::app::{Action, PhotoVaultApp};
use crate::theme;

pub fn show(app: &mut PhotoVaultApp, ui: &mut egui::Ui) {
    let accent = app.accent();
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::page_bg(ui.visuals())))
        .show(ui, |ui| {
            let anim = app.config.animations;
            let start = app.welcome_at;
            let ctx = ui.ctx().clone();
            let stage = |i: usize| crate::anim::stagger(&ctx, start, i, 0.09, 0.45, anim);
            // Language and theme switches in the top corner.
            ui.add_space(12.0);
            crate::ui::end_row(ui, |ui| {
                ui.add_space(16.0);
                let dark = ui.visuals().dark_mode;
                if ui
                    .button(if dark { "☀" } else { "🌙" })
                    .on_hover_text(t!("top.theme"))
                    .clicked()
                {
                    app.actions.push(Action::ToggleTheme);
                }
                let arabic = crate::ui::bidi::vis("العربية");
                let mut lang = app.config.language;
                if crate::ui::segmented(
                    ui,
                    "welcome_lang",
                    &mut lang,
                    &[
                        (photovault::config::Language::En, "English"),
                        (photovault::config::Language::Ar, &arabic),
                    ],
                    accent,
                ) {
                    app.config.language = lang;
                    photovault::i18n::set_language(lang);
                    app.new_archive_name =
                        photovault::i18n::tr_logical("welcome.default_name").to_owned();
                    app.config_dirty = true;
                }
            });
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    // Logo pops in with a slight overshoot and a soft halo.
                    let (slot, _) =
                        ui.allocate_exact_size(egui::vec2(96.0, 96.0), egui::Sense::hover());
                    let pop =
                        crate::anim::ease_out_back(crate::anim::progress(&ctx, start, 0.6, anim));
                    let glow = (ctx.input(|i| i.time) as f32 * 1.6).sin() * 0.5 + 0.5;
                    if anim {
                        ctx.request_repaint_after(std::time::Duration::from_millis(33));
                    }
                    ui.painter().circle_filled(
                        slot.center(),
                        44.0 * pop + glow * 4.0,
                        accent.gamma_multiply(0.10 + 0.06 * glow),
                    );
                    let logo = egui::Rect::from_center_size(
                        slot.center(),
                        egui::vec2(72.0, 72.0) * pop.max(0.01),
                    );
                    theme::paint_logo(ui.painter(), logo, accent);
                    ui.add_space(10.0);
                    fade_up(ui, stage(1), |ui| {
                        ui.label(RichText::new(t!("welcome.title")).size(32.0).strong());
                    });
                    fade_up(ui, stage(2), |ui| {
                        ui.label(
                            RichText::new(t!("welcome.subtitle"))
                                .color(theme::muted(ui))
                                .size(15.0),
                        );
                    });
                    ui.add_space(28.0);
                    if let Some(p) = &app.opening {
                        ui.spinner();
                        ui.label(t!("welcome.opening", path = p.display()));
                    }
                });
                if app.opening.is_some() {
                    return;
                }
                let card_w = 360.0_f32;
                let total = card_w * 2.0 + 24.0;
                let avail = ui.available_width();
                let two_cols = avail >= total + 32.0;
                crate::ui::row(ui, |ui| {
                    if two_cols {
                        ui.add_space((avail - total) / 2.0);
                    } else {
                        ui.add_space(16.0);
                    }
                    if two_cols {
                        fade_up(ui, stage(3), |ui| create_card(app, ui, card_w, accent));
                        ui.add_space(24.0);
                        fade_up(ui, stage(4), |ui| open_card(app, ui, card_w, accent));
                    } else {
                        ui.vertical(|ui| {
                            fade_up(ui, stage(3), |ui| create_card(app, ui, card_w, accent));
                            ui.add_space(16.0);
                            fade_up(ui, stage(4), |ui| open_card(app, ui, card_w, accent));
                        });
                    }
                });
                if !app.config.recent_archives.is_empty() {
                    ui.add_space(28.0);
                    ui.vertical_centered(|ui| {
                        ui.multiply_opacity(stage(5));
                        ui.label(RichText::new(t!("welcome.recent")).strong());
                        for p in app.config.recent_archives.clone() {
                            let label = crate::ui::bidi::vis(&p.display().to_string());
                            if ui.link(label).clicked() {
                                app.actions.push(Action::OpenArchive(p));
                            }
                        }
                    });
                }
            });
        });
}

fn open_card(app: &mut PhotoVaultApp, ui: &mut egui::Ui, card_w: f32, accent: egui::Color32) {
    theme::card(ui).show(ui, |ui| {
        ui.with_layout(crate::ui::column_layout(), |ui| {
            ui.set_width(card_w);
            ui.set_max_width(card_w);
            ui.set_min_height(250.0);
            ui.label(RichText::new("📂").size(30.0).color(accent));
            ui.label(RichText::new(t!("welcome.open")).size(20.0).strong());
            let c = theme::muted(ui);
            crate::ui::bidi::para(ui, t!("welcome.open_body"), 14.5, Some(c));
            ui.add_space(16.0);
            if theme::primary_button(ui, accent, t!("welcome.open_button")).clicked() {
                app.actions.push(Action::PickArchiveFolder);
            }
            ui.add_space(6.0);
            if ui.button(t!("welcome.restore_button")).clicked() {
                app.actions.push(Action::Restore);
            }
        });
    });
}

fn create_card(app: &mut PhotoVaultApp, ui: &mut egui::Ui, card_w: f32, accent: egui::Color32) {
    theme::card(ui).show(ui, |ui| {
        ui.with_layout(crate::ui::column_layout(), |ui| {
            ui.set_width(card_w);
            ui.set_max_width(card_w);
            ui.set_min_height(250.0);
            ui.label(RichText::new("+").size(30.0).color(accent));
            ui.label(RichText::new(t!("welcome.create")).size(20.0).strong());
            let c = theme::muted(ui);
            crate::ui::bidi::para(ui, t!("welcome.create_body"), 14.5, Some(c));
            ui.add_space(10.0);
            ui.label(t!("welcome.archive_name"));
            ui.add(
                crate::ui::field(&mut app.new_archive_name, false, 14.5)
                    .desired_width(f32::INFINITY),
            );
            ui.label(t!("welcome.location"));
            let path = app.new_archive_path.display().to_string();
            ui.add(
                egui::Label::new(RichText::new(&path).small().color(theme::muted(ui))).truncate(),
            )
            .on_hover_text(path);
            if ui.button(t!("welcome.change_location")).clicked()
                && let Some(dir) = rfd::FileDialog::new()
                    .set_title(photovault::i18n::tr_logical("dialog.pick_location"))
                    .pick_folder()
            {
                app.new_archive_path = dir.join("PhotoVault Archive");
            }
            ui.add_space(8.0);
            if theme::primary_button(ui, accent, t!("welcome.create_button")).clicked() {
                app.actions.push(Action::CreateArchiveAt(
                    app.new_archive_path.clone(),
                    app.new_archive_name.clone(),
                ));
            }
        });
    });
}

/// Runs `add` faded in and shifted up by `(1 - t)`.
fn fade_up<R>(ui: &mut egui::Ui, t: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.multiply_opacity(t);
        // Rise by up to 12 px; the total height stays constant so nothing below jumps.
        let vertical = ui.layout().is_vertical();
        let offset = if vertical { (1.0 - t) * 12.0 } else { 0.0 };
        if vertical {
            ui.add_space(offset);
        }
        let r = add(ui);
        if vertical {
            ui.add_space(12.0 - offset);
        }
        r
    })
    .inner
}
