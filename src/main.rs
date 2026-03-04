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

    // On Windows, the default wgpu backend (DX12) reports only `Opaque` alpha mode,
    // so transparent window pixels render as black. Vulkan supports
    // `VK_COMPOSITE_ALPHA_PRE_MULTIPLIED_BIT_KHR`, which lets DWM composite the
    // window correctly. We probe for Vulkan at startup and fall back to the default
    // (DX12) only if no Vulkan adapter is found — the app still runs without
    // transparency in that case.
    #[cfg(target_os = "windows")]
    {
        let vulkan_instance = eframe::wgpu::Instance::new(eframe::wgpu::InstanceDescriptor {
            backends: eframe::wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let has_vulkan = vulkan_instance
            .enumerate_adapters(eframe::wgpu::Backends::VULKAN)
            .next()
            .is_some();

        if has_vulkan {
            tracing::info!("Vulkan available — using Vulkan backend for window transparency");
            native_options.wgpu_options = eframe::egui_wgpu::WgpuConfiguration {
                supported_backends: eframe::wgpu::Backends::VULKAN,
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
