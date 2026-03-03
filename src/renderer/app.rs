//! Main application

use eframe::egui;

use crate::config::AppSettings;
use crate::core::{DataCollector, TelemetryBuffer};
use crate::plugins::GamePlugin;
use crate::renderer::{SteeringWheel, TraceGraph};
use std::sync::Arc;

/// Main SimTrace application
pub struct SimTraceApp {
    /// Application settings
    settings: AppSettings,
    /// Data collector
    collector: DataCollector,
    /// Active plugin name
    active_plugin: Option<String>,
    /// Connection status
    connected: bool,
    /// Current telemetry data
    current_steering: f32,
    current_abs_active: bool,
}

impl SimTraceApp {
    /// Create a new SimTrace application
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Configure egui
        configure_egui(&cc.egui_ctx);

        let settings = AppSettings::default();
        let collector_config = crate::core::collector::CollectorConfig {
            update_rate_hz: settings.collector.update_rate_hz,
            buffer_window_secs: settings.collector.buffer_window_secs.unwrap_or(10),
        };
        let collector = DataCollector::new(collector_config);

        Self {
            settings,
            collector,
            active_plugin: None,
            connected: false,
            current_steering: 0.0,
            current_abs_active: false,
        }
    }

    /// Activate a plugin
    pub fn activate_plugin(&mut self, plugin_name: &str) {
        if let Err(e) = self.collector.activate_plugin(plugin_name) {
            tracing::error!("Failed to activate plugin: {}", e);
            return;
        }

        self.active_plugin = Some(plugin_name.to_string());
        self.connected = self.collector.is_connected();
    }

    /// Disconnect from current plugin
    pub fn disconnect(&mut self) {
        self.connected = false;
        self.active_plugin = None;
    }

    /// Get the telemetry buffer
    fn buffer(&self) -> Arc<TelemetryBuffer> {
        self.collector.buffer()
    }
}

impl eframe::App for SimTraceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll telemetry from the game plugin
        self.collector.poll();

        // Update current telemetry display
        let buffer = self.buffer();
        if let Some(point) = buffer.latest() {
            self.current_steering = point.telemetry.steering_angle;
            self.current_abs_active = point.abs_active;
        }

        // Main UI
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("SimTrace");
                ui.separator();

                // Plugin selection
                ui.label("Plugin:");
                if let Some(plugin) = &self.active_plugin {
                    ui.label(plugin);
                    if ui.button("Disconnect").clicked() {
                        self.disconnect();
                    }
                } else {
                    let available: Vec<_> = self.collector.available_plugins().to_vec();
                    if available.is_empty() {
                        ui.label("No plugins available");
                    } else {
                        for plugin in available {
                            if ui.button(&plugin).clicked() {
                                self.activate_plugin(&plugin);
                            }
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let status_color = if self.connected {
                        egui::Color32::GREEN
                    } else {
                        egui::Color32::RED
                    };
                    ui.label(
                        egui::RichText::new(if self.connected {
                            "Connected"
                        } else {
                            "Disconnected"
                        })
                        .color(status_color),
                    );
                });
            });
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Time window:");
                if ui.button("-2s").clicked() {
                    let new_window = (self.settings.graph.window_seconds - 2.0).max(2.0);
                    self.settings.graph.window_seconds = new_window;
                    self.collector
                        .buffer()
                        .set_window_duration(std::time::Duration::from_secs_f64(new_window));
                }
                ui.label(format!("{:.1}s", self.settings.graph.window_seconds));
                if ui.button("+2s").clicked() {
                    let new_window = self.settings.graph.window_seconds + 2.0;
                    self.settings.graph.window_seconds = new_window;
                    self.collector
                        .buffer()
                        .set_window_duration(std::time::Duration::from_secs_f64(new_window));
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                // Top: Trace graph
                ui.label("Telemetry Trace");
                let graph_size = ui.available_size_before_wrap();
                let graph_size = egui::Vec2::new(graph_size.x, 300.0);

                TraceGraph::new(
                    &self.collector.buffer(),
                    &self.settings.graph,
                    &self.settings.colors,
                )
                .show(ui, graph_size);

                // Bottom: Steering wheel and current values
                ui.horizontal(|ui| {
                    // Steering wheel
                    ui.vertical(|ui| {
                        ui.label("Steering");
                        let wheel_size = egui::Vec2::new(250.0, 250.0);
                        let max_angle = self
                            .collector
                            .active_plugin()
                            .map(|p: &dyn GamePlugin| p.get_config().max_steering_angle)
                            .unwrap_or(900.0);

                        SteeringWheel::new(&self.settings.steering_wheel, max_angle).show(
                            ui,
                            self.current_steering,
                            wheel_size,
                        );
                    });

                    // Current values
                    ui.vertical(|ui| {
                        ui.separator();
                        ui.vertical_centered(|ui| {
                            ui.label(format!("Steering: {:.0}°", self.current_steering));
                            ui.label(format!(
                                "ABS: {}",
                                if self.current_abs_active {
                                    "ACTIVE"
                                } else {
                                    "OFF"
                                }
                            ));
                        });
                    });
                });
            });
        });
    }
}

/// Configure egui settings
fn configure_egui(ctx: &egui::Context) {
    let style = (*ctx.style()).clone();
    ctx.set_style(style);
}
