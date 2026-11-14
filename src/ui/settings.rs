//! Settings dialog content.

use eframe::egui::{self, RichText};
use photovault::config::{Language, ThemeMode, config_dir, log_path};
use photovault::fsutil::human_bytes;
use photovault::t;

use crate::app::{Action, PhotoVaultApp};
use crate::theme::{self, ACCENTS};
use crate::ui::{row, segmented};

const LABEL_W: f32 = 170.0;

/// One labelled settings row (label on the reading-start side).
fn setting(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    row(ui, |ui| {
        let h = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(egui::vec2(LABEL_W, h), crate::ui::row_layout(), |ui| {
            ui.set_min_width(LABEL_W);
            ui.label(label);
        });
        add(ui);
    });
    ui.add_space(6.0);
}

pub fn show(app: &mut PhotoVaultApp, ui: &mut egui::Ui, trash_size: Option<u64>, keep: &mut bool) {
    let accent = app.accent();
    ui.label(RichText::new(t!("settings.title")).size(20.0).strong());
    ui.add_space(10.0);
    let mut changed = false;
    let mut language_changed = false;

    setting(ui, t!("settings.language"), |ui| {
        let arabic = crate::ui::bidi::vis("العربية");
        language_changed |= segmented(
            ui,
            "language",
            &mut app.config.language,
            &[(Language::En, "English"), (Language::Ar, &arabic)],
            accent,
        );
    });
    setting(ui, t!("settings.theme"), |ui| {
        changed |= segmented(
            ui,
            "theme",
            &mut app.config.theme,
            &[
                (ThemeMode::Light, t!("settings.light")),
                (ThemeMode::Dark, t!("settings.dark")),
                (ThemeMode::System, t!("settings.system")),
            ],
            accent,
        );
    });
    setting(ui, t!("settings.accent"), |ui| {
        for (i, (name, color)) in ACCENTS.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
            let hover = ui.ctx().animate_bool_with_time(
                egui::Id::unique(("accent_hover", i)),
                resp.hovered(),
                0.12,
            );
            ui.painter()
                .circle_filled(rect.center(), 10.0 + hover * 1.5, *color);
            if app.config.accent == i {
                ui.painter().circle_stroke(
                    rect.center(),
                    12.5,
                    egui::Stroke::new(2.0, ui.visuals().text_color()),
                );
            }
            if resp.on_hover_text(*name).clicked() {
                app.config.accent = i;
                changed = true;
            }
        }
    });
    setting(ui, t!("settings.reminder"), |ui| {
        changed |= segmented(
            ui,
            "reminder",
            &mut app.config.backup_reminder_days,
            &[
                (0, t!("settings.off")),
                (7, t!("settings.7")),
                (30, t!("settings.30")),
                (90, t!("settings.90")),
            ],
            accent,
        );
    });
    setting(ui, t!("settings.thumb_size"), |ui| {
        changed |= ui
            .add(egui::Slider::new(&mut app.config.thumb_size, 90.0..=320.0).suffix(" px"))
            .changed();
    });
    setting(ui, t!("settings.ui_scale"), |ui| {
        let r = ui.add(egui::Slider::new(&mut app.config.ui_scale, 0.8..=1.6).step_by(0.05));
        if r.drag_stopped() || (r.changed() && !r.dragged()) {
            ui.ctx().set_zoom_factor(app.config.ui_scale);
            app.config_dirty = true;
        }
    });
    setting(ui, t!("settings.slideshow"), |ui| {
        if ui
            .add(egui::Slider::new(&mut app.config.slideshow_secs, 1.0..=20.0).suffix(" s"))
            .changed()
        {
            app.config_dirty = true;
        }
    });
    setting(ui, t!("settings.animations"), |ui| {
        if crate::ui::toggle(ui, &mut app.config.animations, accent).changed() {
            app.config_dirty = true;
        }
    });
    setting(ui, t!("settings.confirm_delete"), |ui| {
        if crate::ui::toggle(ui, &mut app.config.confirm_before_delete, accent).changed() {
            app.config_dirty = true;
        }
    });
    setting(ui, t!("settings.fonts"), |ui| {
        ui.label(
            RichText::new(t!("settings.fonts_value"))
                .small()
                .color(theme::muted(ui)),
        );
    });
    setting(ui, t!("settings.trash"), |ui| {
        match trash_size {
            Some(n) => ui.label(human_bytes(n)),
            None => ui.spinner(),
        };
        if ui
            .add_enabled(
                trash_size.is_some_and(|n| n > 0),
                egui::Button::new(t!("settings.empty_trash")),
            )
            .clicked()
            && let Some(a) = app.archive.as_ref()
        {
            match a.empty_trash() {
                Ok(()) => app.actions.push(Action::Toast(
                    t!("settings.trash_emptied").to_owned(),
                    crate::ui::toasts::ToastKind::Success,
                )),
                Err(e) => app.actions.push(Action::Toast(
                    photovault::bidi::visual_string(&e.to_string()),
                    crate::ui::toasts::ToastKind::Error,
                )),
            }
            app.toasts.retain(|t| t.undo.is_none());
            *keep = false;
        }
    });

    if language_changed {
        photovault::i18n::set_language(app.config.language);
        app.generation += 1;
        app.config_dirty = true;
    }
    if changed {
        app.apply_theme_animated(ui.ctx());
        app.config_dirty = true;
    }
    ui.add_space(8.0);
    row(ui, |ui| {
        if ui.button(t!("settings.open_config")).clicked() {
            let dir = config_dir();
            let _ = std::fs::create_dir_all(&dir);
            app.actions.push(Action::Open(dir));
        }
        if ui.button(t!("settings.open_log")).clicked() {
            app.actions.push(Action::Open(log_path()));
        }
        if ui.button(t!("settings.shortcuts")).clicked() {
            app.dialog = Some(crate::app::Dialog::Shortcuts);
            *keep = false;
        }
        if ui.button(t!("intro.replay")).clicked() {
            app.intro = Some(crate::ui::intro::IntroState::new());
            *keep = false;
        }
    });
    ui.add_space(10.0);
    crate::ui::end_row(ui, |ui| {
        if theme::primary_button(ui, accent, t!("common.done")).clicked() {
            *keep = false;
        }
    });
}
