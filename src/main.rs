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

    // Create native options
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    // Run the application
    eframe::run_native(
        "SimTrace",
        native_options,
        Box::new(|cc| Ok(Box::new(SimTraceApp::new(cc)))),
    )
}
