//! SimTrace - Sim racing telemetry visualization

mod config;
mod core;
mod plugins;
mod renderer;

use eframe::egui;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use renderer::SimTraceApp;

fn main() -> eframe::Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "simtrace=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load settings to restore last window geometry
    let saved = config::AppSettings::load_or_default();

    // Create native options — main window IS the transparent overlay
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([saved.overlay.width, saved.overlay.height])
            .with_position(egui::pos2(saved.overlay.position_x, saved.overlay.position_y))
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(true),
        ..Default::default()
    };

    // Run the application
    eframe::run_native(
        "SimTrace",
        native_options,
        Box::new(|cc| Ok(Box::new(SimTraceApp::new(cc)))),
    )
}
