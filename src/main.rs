//! PhotoVault desktop app entry point.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod anim;
mod app;
mod jobs;
mod theme;
mod thumbs;
mod ui;

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

use eframe::egui;
use photovault::config::{AppConfig, config_dir, log_path};

/// Writes log lines to stderr and to the log file.
struct Tee {
    file: Option<Mutex<File>>,
}

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Some(f) = &self.file
            && let Ok(mut f) = f.lock()
        {
            let _ = f.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(f) = &self.file
            && let Ok(mut f) = f.lock()
        {
            let _ = f.flush();
        }
        Ok(())
    }
}

/// Sets up logging with a simple size-based rotation (`photovault.log` → `.1`).
fn init_logging() {
    let _ = std::fs::create_dir_all(config_dir());
    let path = log_path();
    if std::fs::metadata(&path)
        .map(|m| m.len() > 2 * 1024 * 1024)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
        .map(Mutex::new);
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .default_filter_or("info,wgpu=warn,naga=warn,eframe=warn,egui_wgpu=warn"),
    )
    .target(env_logger::Target::Pipe(Box::new(Tee { file })))
    .format_timestamp_secs()
    .init();
}

fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../assets/icon-256.png"))
        .ok()?
        .to_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

fn main() -> eframe::Result<()> {
    init_logging();
    log::info!("PhotoVault {} starting", photovault::APP_VERSION);
    let config = AppConfig::load();
    let size = config.window_size.unwrap_or([1280.0, 820.0]);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("PhotoVault")
        .with_app_id("photovault")
        .with_inner_size([size[0].max(1000.0), size[1].max(650.0)])
        .with_min_inner_size([1000.0, 650.0])
        .with_drag_and_drop(true);
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "PhotoVault",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_zoom_factor(config.ui_scale.clamp(0.8, 1.6));
            Ok(Box::new(app::PhotoVaultApp::new(cc)))
        }),
    )
}
