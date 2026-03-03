//! Overlay widget for telemetry display

use eframe::egui;

use crate::config::AppSettings;
use crate::core::DataCollector;
use crate::plugins::GamePlugin;
use crate::renderer::{SteeringWheel, TraceGraph};
use std::sync::Arc;

/// Overlay widget showing telemetry in a compact, configurable window
pub struct OverlayWidget {
    /// Current settings
    settings: AppSettings,
    /// Data collector reference
    collector: Arc<DataCollector>,
    /// Current steering angle
    current_steering: f32,
    /// Current ABS state
    current_abs_active: bool,
}

impl OverlayWidget {
    /// Create a new overlay widget
    pub fn new(settings: AppSettings, collector: Arc<DataCollector>) -> Self {
        Self {
            settings,
            collector,
            current_steering: 0.0,
            current_abs_active: false,
        }
    }

    /// Update settings from config
    pub fn update_settings(&mut self, new_settings: AppSettings) {
        self.settings = new_settings;
    }

    /// Update current telemetry values
    pub fn update_telemetry(&mut self) {
        let buffer = self.collector.buffer();
        if let Some(point) = buffer.latest() {
            self.current_steering = point.telemetry.steering_angle;
            self.current_abs_active = point.abs_active;
        }
    }

    /// Show the overlay widget
    pub fn show(&mut self, ctx: &egui::Context) {
        // Copy overlay settings to avoid borrow issues
        let overlay_settings = self.settings.overlay.clone();

        // Build the window with overlay settings - no borders, transparent
        egui::Window::new("")
            .default_pos([overlay_settings.position_x, overlay_settings.position_y])
            .default_size([overlay_settings.width, overlay_settings.height])
            .resizable(true)
            .movable(true)
            .collapsible(false)
            .title_bar(false)
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::TRANSPARENT)
                    .shadow(egui::epaint::Shadow::NONE),
            )
            .anchor(egui::Align2::LEFT_TOP, [0.0, 0.0])
            .show(ctx, |ui| {
                // Apply semi-transparent background
                let alpha = ((1.0 - overlay_settings.opacity) * 255.0) as u8;
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    4.0,
                    egui::Color32::from_black_alpha(alpha),
                );

                // Main content
                ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                    // Top bar with controls
                    self.show_top_bar(ui);

                    // Main content area
                    self.show_content(ui);
                });
            });
    }

    /// Show the top control bar
    fn show_top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Title
            ui.heading(egui::RichText::new("🏁").small());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Drag handle indicator
                ui.label(egui::RichText::new("☰").small().weak());
            });
        });

        ui.add_space(4.0);
    }

    /// Show the main telemetry content
    fn show_content(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            // Trace graph
            let graph_size = ui.available_size_before_wrap();
            let graph_height = (graph_size.y * 0.6).max(150.0);
            let graph_size = egui::Vec2::new(graph_size.x, graph_height);

            TraceGraph::new(
                &self.collector.buffer(),
                &self.settings.graph,
                &self.settings.colors,
                self.settings.overlay.opacity,
            )
            .show(ui, graph_size);

            ui.add_space(8.0);

            // Bottom row: steering wheel and current values
            ui.horizontal(|ui| {
                // Steering wheel
                ui.vertical(|ui| {
                    let wheel_size = egui::Vec2::new(
                        (ui.available_width() * 0.4).min(150.0),
                        (ui.available_width() * 0.4).min(150.0),
                    );
                    let center = egui::pos2(
                        ui.next_widget_position().x + wheel_size.x / 2.0,
                        ui.next_widget_position().y + wheel_size.y / 2.0,
                    );
                    let (rect, _) = ui.allocate_exact_size(wheel_size, egui::Sense::hover());
                    SteeringWheel::draw(
                        ui.painter(),
                        rect.center(),
                        wheel_size.x / 2.0 - 6.0,
                        self.current_steering,
                        1.0,
                    );
                    let _ = center;
                });

                // Current values
                ui.vertical(|ui| {
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("Steering").small().weak());
                    ui.label(
                        egui::RichText::new(format!("{:.0}°", self.current_steering))
                            .small()
                            .monospace(),
                    );

                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("ABS").small().weak());
                    let abs_text = if self.current_abs_active {
                        egui::RichText::new("ACTIVE").small().monospace().color(
                            egui::Color32::from_hex("#FFA500").unwrap_or(egui::Color32::ORANGE),
                        )
                    } else {
                        egui::RichText::new("OFF").small().monospace().weak()
                    };
                    ui.label(abs_text);
                });

                // Spacer
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Time window controls
                    ui.label(egui::RichText::new("Window").small().weak());
                    if ui.button("-").clicked() {
                        let new_window = (self.settings.graph.window_seconds - 1.0).max(2.0);
                        self.settings.graph.window_seconds = new_window;
                        self.collector
                            .buffer()
                            .set_window_duration(std::time::Duration::from_secs_f64(new_window));
                    }
                    ui.label(
                        egui::RichText::new(format!("{:.1}s", self.settings.graph.window_seconds))
                            .small()
                            .monospace(),
                    );
                    if ui.button("+").clicked() {
                        let new_window = self.settings.graph.window_seconds + 1.0;
                        self.settings.graph.window_seconds = new_window;
                        self.collector
                            .buffer()
                            .set_window_duration(std::time::Duration::from_secs_f64(new_window));
                    }
                });
            });
        });
    }

    /// Get mutable reference to the data collector
    pub fn collector(&mut self) -> &mut DataCollector {
        Arc::get_mut(&mut self.collector).expect("Collector should be uniquely owned by overlay")
    }
}
