//! Small animation helpers (eased timings, view transitions, theme cross-fade).

use std::time::Instant;

use eframe::egui;

use crate::app::PhotoVaultApp;

/// Ease-out cubic.
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Ease-out with a gentle overshoot ("back"), for pops.
pub fn ease_out_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

/// Progress `0..=1` of an animation that started at `start` and lasts `secs`
/// (instantly 1 when animations are disabled). Requests repaints while running.
pub fn progress(ctx: &egui::Context, start: Instant, secs: f32, enabled: bool) -> f32 {
    if !enabled {
        return 1.0;
    }
    let t = start.elapsed().as_secs_f32() / secs;
    if t < 1.0 {
        ctx.request_repaint();
    }
    t.min(1.0)
}

/// Staggered entrance: item `i` starts `delay` seconds after the previous one.
pub fn stagger(
    ctx: &egui::Context,
    start: Instant,
    i: usize,
    delay: f32,
    secs: f32,
    enabled: bool,
) -> f32 {
    if !enabled {
        return 1.0;
    }
    let t = (start.elapsed().as_secs_f32() - i as f32 * delay) / secs;
    if t < 1.0 {
        ctx.request_repaint();
    }
    ease_out(t.max(0.0))
}

/// Eased progress of the current central-view transition.
pub fn view_transition(app: &mut PhotoVaultApp, ctx: &egui::Context) -> f32 {
    if app.anim_view != Some(app.view) {
        app.anim_view = Some(app.view);
        app.view_changed_at = Instant::now();
    }
    ease_out(progress(
        ctx,
        app.view_changed_at,
        0.28,
        app.config.animations,
    ))
}

/// Linear blend between two colors.
pub fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}
