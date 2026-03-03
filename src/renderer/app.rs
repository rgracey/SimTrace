//! Main application

use eframe::egui;

use crate::config::AppSettings;
use crate::core::DataCollector;
use std::sync::{Arc, Mutex};

/// Main SimTrace application
pub struct SimTraceApp {
    /// Settings
    settings: AppSettings,
    /// Data collector
    collector: Option<Arc<Mutex<DataCollector>>>,
    /// Current steering angle
    current_steering: f32,
    /// Current ABS state
    current_abs_active: bool,
    /// Whether overlay viewport is open
    overlay_open: bool,
}

impl SimTraceApp {
    /// Create a new SimTrace application
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Configure egui
        configure_egui(&cc.egui_ctx);

        // Try to load existing settings
        let (settings, _) = load_settings();

        Self {
            settings,
            collector: None,
            current_steering: 0.0,
            current_abs_active: false,
            overlay_open: false,
        }
    }

    /// Toggle overlay viewport
    fn toggle_overlay(&mut self, ctx: &egui::Context) {
        self.overlay_open = !self.overlay_open;

        if self.overlay_open {
            // Create data collector if needed
            if self.collector.is_none() {
                let collector_config = crate::core::collector::CollectorConfig {
                    update_rate_hz: self.settings.collector.update_rate_hz,
                    buffer_window_secs: self.settings.collector.buffer_window_secs.unwrap_or(10),
                };
                let collector = DataCollector::new(collector_config);
                self.collector = Some(Arc::new(Mutex::new(collector)));
            }

            // Open the overlay viewport
            open_overlay_viewport(ctx, &self.settings);
        } else {
            // Close the overlay viewport
            close_overlay_viewport(ctx);
        }
    }

    /// Save settings
    fn save_settings(&self) -> Result<(), anyhow::Error> {
        let config_path = dirs::config_dir()
            .map(|p| p.join("simtrace").join("settings.toml"))
            .or_else(|| dirs::home_dir().map(|p| p.join(".simtrace").join("settings.toml")))
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        self.settings.save(&config_path)?;
        tracing::info!("Settings saved to {:?}", config_path);
        Ok(())
    }
}

impl eframe::App for SimTraceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll telemetry if we have a collector
        if let Some(ref collector) = self.collector {
            if let Ok(mut c) = collector.lock() {
                c.poll();
                let buffer = c.buffer();
                if let Some(point) = buffer.latest() {
                    self.current_steering = point.telemetry.steering_angle;
                    self.current_abs_active = point.abs_active;
                }
            }
        }

        // Config window - always visible in main viewport
        egui::Window::new("⚙️ Config")
            .resizable(true)
            .default_size([350.0, 300.0])
            .anchor(egui::Align2::LEFT_TOP, [10.0, 10.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.heading("SimTrace");
                    ui.separator();

                    // Action buttons
                    ui.horizontal(|ui| {
                        if ui
                            .button(if self.overlay_open {
                                "Hide Overlay"
                            } else {
                                "Show Overlay"
                            })
                            .clicked()
                        {
                            self.toggle_overlay(ctx);
                        }
                        if ui.button("Save").clicked() {
                            if let Err(e) = self.save_settings() {
                                ui.label(
                                    egui::RichText::new(format!("Error: {}", e))
                                        .small()
                                        .color(egui::Color32::RED),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new("✓ Saved!")
                                        .small()
                                        .color(egui::Color32::GREEN),
                                );
                            }
                        }
                    });

                    ui.add_space(10.0);
                    ui.separator();

                    // Quick settings
                    ui.label("Overlay Size");
                    ui.horizontal(|ui| {
                        ui.label("Width:");
                        ui.add(egui::Slider::new(
                            &mut self.settings.overlay.width,
                            300.0..=1200.0,
                        ));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Height:");
                        ui.add(egui::Slider::new(
                            &mut self.settings.overlay.height,
                            200.0..=800.0,
                        ));
                    });

                    ui.add_space(5.0);
                    ui.label("Opacity");
                    ui.horizontal(|ui| {
                        ui.add(egui::Slider::new(
                            &mut self.settings.overlay.opacity,
                            0.1..=1.0,
                        ));
                    });

                    ui.add_space(5.0);
                    ui.label("Time Window");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Slider::new(&mut self.settings.graph.window_seconds, 2.0..=30.0)
                                .suffix("s"),
                        );
                    });
                });
            });
    }
}

/// Load settings from file
fn load_settings() -> (AppSettings, bool) {
    let config_path: Option<std::path::PathBuf> = dirs::config_dir()
        .map(|p| p.join("simtrace").join("settings.toml"))
        .or_else(|| dirs::home_dir().map(|p| p.join(".simtrace").join("settings.toml")));

    if let Some(path) = config_path {
        match AppSettings::load(&path) {
            Ok(settings) => {
                tracing::info!("Settings loaded from {:?}", path);
                return (settings, true);
            }
            Err(e) => {
                tracing::warn!("Failed to load settings from {:?}: {}", path, e);
            }
        }
    }

    (AppSettings::default(), false)
}

/// Overlay viewport ID
fn overlay_viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("simtrace-overlay")
}

/// Open the overlay viewport
fn open_overlay_viewport(ctx: &egui::Context, settings: &AppSettings) {
    let viewport_builder = egui::ViewportBuilder::default()
        .with_title("")
        .with_inner_size([settings.overlay.width, settings.overlay.height])
        .with_position([settings.overlay.position_x, settings.overlay.position_y])
        .with_decorations(false)
        .with_transparent(true);

    ctx.show_viewport_immediate(overlay_viewport_id(), viewport_builder, |ctx, _class| {
        // Configure the overlay viewport
        ctx.set_visuals(egui::Visuals::dark());

        // Draw semi-transparent background
        let alpha = settings.overlay.opacity;
        egui::CentralPanel::default().show(ctx, |ui| {
            let bg_color = egui::Color32::from_black_alpha(((1.0 - alpha) * 255.0) as u8);
            ui.painter()
                .rect_filled(ui.available_rect_before_wrap(), 4.0, bg_color);

            // Content
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                // Drag handle area
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("☰").small().weak());
                    ui.label(egui::RichText::new("🏁 SimTrace").small());
                });
                ui.add_space(4.0);

                // Trace graph
                let graph_size = ui.available_size_before_wrap();
                let graph_height = (graph_size.y * 0.6).max(100.0);
                let graph_size = egui::Vec2::new(graph_size.x, graph_height);

                crate::renderer::TraceGraph::new_simple(&settings.graph, &settings.colors)
                    .show_simple(ui, graph_size);

                ui.add_space(8.0);

                // Bottom row
                ui.horizontal(|ui| {
                    // Steering wheel placeholder
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Steering").small().weak());
                        ui.label(
                            egui::RichText::new(format!("{:.0}°", 0.0))
                                .small()
                                .monospace(),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("Window").small().weak());
                        ui.label(
                            egui::RichText::new(format!("{:.1}s", settings.graph.window_seconds))
                                .small()
                                .monospace(),
                        );
                    });
                });
            });
        });
    });
}

/// Close the overlay viewport
fn close_overlay_viewport(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
}

/// Configure egui settings
fn configure_egui(ctx: &egui::Context) {
    let style = (*ctx.style()).clone();
    ctx.set_style(style);
}
