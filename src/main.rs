//! SimTrace - Sim racing telemetry visualization
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod coach;
mod config;
mod core;
mod plugins;
mod renderer;

use eframe::egui;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use renderer::SimTraceApp;

fn main() -> eframe::Result<()> {
    // Set up logging: always write to file; also write to stderr on non-Windows
    // (on Windows the console is hidden so stderr is discarded).
    let log_dir = config::AppSettings::config_dir();
    let _log_guard = init_logging(log_dir.as_deref());

    // Load settings to restore last window geometry
    let saved = config::AppSettings::load_or_default();

    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([saved.overlay.width, saved.overlay.height])
        .with_position(egui::pos2(
            saved.overlay.position_x,
            saved.overlay.position_y,
        ))
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_resizable(true);

    #[allow(unused_mut)]
    let mut native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    // On Windows, the default wgpu backend (DX12) reports only `Opaque` composite
    // alpha, so transparent pixels render black. Vulkan exposes
    // `VK_COMPOSITE_ALPHA_PRE_MULTIPLIED_BIT_KHR`, which lets DWM composite the
    // window correctly. Probe for a Vulkan adapter at startup; fall back to DX12
    // (no transparency) only if none is found.
    #[cfg(target_os = "windows")]
    {
        let vulkan_instance = eframe::wgpu::Instance::new(&eframe::wgpu::InstanceDescriptor {
            backends: eframe::wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let has_vulkan = !vulkan_instance
            .enumerate_adapters(eframe::wgpu::Backends::VULKAN)
            .is_empty();

        if has_vulkan {
            tracing::info!("Vulkan available — using Vulkan backend for window transparency");
            native_options.wgpu_options = eframe::egui_wgpu::WgpuConfiguration {
                wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(
                    eframe::egui_wgpu::WgpuSetupCreateNew {
                        instance_descriptor: eframe::wgpu::InstanceDescriptor {
                            backends: eframe::wgpu::Backends::VULKAN,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                ),
                ..Default::default()
            };
        } else {
            tracing::warn!(
                "Vulkan unavailable — falling back to DX12; window transparency will not work"
            );
        }
    }

    // Run the application
    eframe::run_native(
        "SimTrace",
        native_options,
        Box::new(|cc| Ok(Box::new(SimTraceApp::new(cc)))),
    )
}

/// Initialise tracing: file sink always, stderr sink on non-Windows.
/// Returns the worker guard that must be kept alive for the duration of the process.
fn init_logging(
    log_dir: Option<&std::path::Path>,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "simtrace=info".into());

    match log_dir.and_then(|d| {
        std::fs::create_dir_all(d).ok()?;
        Some(d.to_path_buf())
    }) {
        Some(dir) => {
            let appender = tracing_appender::rolling::daily(&dir, "simtrace.log");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            tracing_subscriber::registry()
                .with(filter)
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(writer)
                        .with_ansi(false),
                )
                .init();
            Some(guard)
        }
        None => {
            // No writable config dir — fall back to stderr only
            tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer())
                .init();
            None
        }
    }
}
