#![windows_subsystem = "windows"]

mod app;
mod assets;
mod audio;
mod config;
mod domain;
mod easing;
mod logutil;
mod platform;
mod tray;
mod ui;
mod user_config;
mod viewport;

use eframe::egui;

use crate::app::EyeSaverApp;
use crate::config::IDLE_WINDOW_SIZE;
use crate::platform::{single_instance_acquire, suppress_window_chrome};

fn main() -> eframe::Result {
    logutil::init();
    if !single_instance_acquire() {
        return Ok(());
    }
    logutil::log("main: starting eframe");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_mouse_passthrough(true)
            .with_taskbar(false)
            .with_has_shadow(false)
            .with_inner_size(IDLE_WINDOW_SIZE)
            .with_min_inner_size(IDLE_WINDOW_SIZE)
            .with_max_inner_size(IDLE_WINDOW_SIZE)
            .with_position([-32.0, -32.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Blink",
        options,
        Box::new(|cc| {
            logutil::log("main: creating EyeSaverApp");
            suppress_window_chrome(cc);
            Ok(Box::new(EyeSaverApp::new(&cc.egui_ctx)))
        }),
    )
}
