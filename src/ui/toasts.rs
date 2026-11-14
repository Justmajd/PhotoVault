//! Non-blocking toast notifications (bottom-right), with optional Undo.

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui::{self, Color32, RichText};
use photovault::archive::TrashBatch;
use photovault::t;

use crate::app::{Action, PhotoVaultApp};
use crate::theme;

/// Visual style of a toast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

/// One toast.
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub kind: ToastKind,
    pub created: Instant,
    pub secs: f32,
    pub undo: Option<TrashBatch>,
    pub reveal: Option<PathBuf>,
    /// Shows an "Update" button that opens Settings.
    pub open_settings: bool,
}

impl Toast {
    pub fn new(id: u64, text: String, kind: ToastKind) -> Self {
        let secs = match kind {
            ToastKind::Error => 8.0,
            ToastKind::Warning => 6.0,
            _ => 4.0,
        };
        Self {
            id,
            text,
            kind,
            created: Instant::now(),
            secs,
            undo: None,
            reveal: None,
            open_settings: false,
        }
    }
}

pub fn show(app: &mut PhotoVaultApp, ctx: &egui::Context) {
    app.toasts
        .retain(|t| t.created.elapsed().as_secs_f32() < t.secs);
    if app.toasts.is_empty() {
        return;
    }
    let mut dismiss = Vec::new();
    let animations = app.config.animations;
    let mut animating = false;
    egui::Area::new(egui::Id::unique("toasts"))
        .order(egui::Order::Tooltip)
        .anchor(
            if crate::ui::rtl() {
                egui::Align2::LEFT_BOTTOM
            } else {
                egui::Align2::RIGHT_BOTTOM
            },
            egui::vec2(if crate::ui::rtl() { 18.0 } else { -18.0 }, -38.0),
        )
        .show(ctx, |ui| {
            ui.set_max_width(380.0);
            let outer = ui;
            for toast in app.toasts.iter().rev().take(5) {
                let ui = &mut *outer;
                let color = match toast.kind {
                    ToastKind::Info => app_accent(ui),
                    ToastKind::Success => theme::SUCCESS,
                    ToastKind::Warning => theme::AMBER,
                    ToastKind::Error => theme::DANGER,
                };
                // Fade/slide in, fade out before expiring.
                let age = toast.created.elapsed().as_secs_f32();
                let left = toast.secs - age;
                let alpha = if animations {
                    crate::anim::ease_out(age / 0.25).min((left / 0.35).clamp(0.0, 1.0))
                } else {
                    1.0
                };
                if alpha < 1.0 {
                    animating = true;
                }
                ui.add_space((1.0 - alpha) * 10.0);
                let mut frame_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(ui.available_rect_before_wrap())
                        .layout(*ui.layout()),
                );
                frame_ui.multiply_opacity(alpha);
                let ui = &mut frame_ui;
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.7)))
                    .show(ui, |ui| {
                        crate::ui::row(ui, |ui| {
                            let (r, _) =
                                ui.allocate_exact_size(egui::vec2(4.0, 22.0), egui::Sense::hover());
                            ui.painter()
                                .rect_filled(r, egui::CornerRadius::same(2), color);
                            if photovault::bidi::has_rtl(&toast.text) {
                                ui.allocate_ui(egui::vec2(230.0, 0.0), |ui| {
                                    crate::ui::bidi::para(ui, &toast.text, 14.5, None)
                                });
                            } else {
                                ui.add(egui::Label::new(&toast.text).wrap());
                            }
                            if toast.undo.is_some() {
                                let left = (toast.secs - toast.created.elapsed().as_secs_f32())
                                    .max(0.0)
                                    .ceil();
                                if ui
                                    .button(RichText::new(t!("toast.undo", s = left)).strong())
                                    .clicked()
                                {
                                    app.actions.push(Action::Undo(toast.id));
                                }
                            }
                            if let Some(p) = &toast.reveal
                                && ui.button(t!("toast.show")).clicked()
                            {
                                app.actions.push(Action::Reveal(p.clone()));
                            }
                            if toast.open_settings && ui.button(t!("update.open")).clicked() {
                                app.actions.push(Action::OpenSettings);
                                dismiss.push(toast.id);
                            }
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("×").color(Color32::GRAY))
                                        .frame(false),
                                )
                                .clicked()
                            {
                                dismiss.push(toast.id);
                            }
                        });
                    });
                let used = frame_ui.min_rect();
                outer.allocate_rect(used, egui::Sense::hover());
                outer.add_space(6.0);
            }
        });
    app.toasts.retain(|t| !dismiss.contains(&t.id));
    ctx.request_repaint_after(std::time::Duration::from_millis(if animating {
        16
    } else {
        500
    }));
}

fn app_accent(ui: &egui::Ui) -> Color32 {
    ui.visuals().hyperlink_color
}
