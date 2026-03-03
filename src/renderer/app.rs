//! Main application

use eframe::egui;

use crate::config::{AppSettings, ConfigWindow};
use crate::core::DataCollector;
use crate::renderer::OverlayWidget;
use std::sync::Arc;

/// Main SimTrace application
pub struct SimTraceApp {
    /// Configuration window
    config_window: ConfigWindow,
    /// Overlay widget
    overlay_widget: Option<OverlayWidget>,
    /// Whether overlay is visible
    overlay_visible: bool,
}

impl SimTraceApp {
    /// Create a new SimTrace application
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Configure egui
        configure_egui(&cc.egui_ctx);

        // Try to load existing settings
        let (settings, _) = load_settings();

        // Create config window with loaded settings
        let mut config_window = ConfigWindow::new();
        config_window.set_settings(settings.clone());

        Self {
            config_window,
            overlay_widget: None,
            overlay_visible: false,
        }
    }

    /// Toggle overlay visibility
    fn toggle_overlay(&mut self) {
        self.overlay_visible = !self.overlay_visible;

        if self.overlay_visible {
            // Create overlay if it doesn't exist
            if self.overlay_widget.is_none() {
                let settings = self.config_window.settings();

                // Create data collector
                let collector_config = crate::core::collector::CollectorConfig {
                    update_rate_hz: settings.collector.update_rate_hz,
                    buffer_window_secs: settings.collector.buffer_window_secs.unwrap_or(10),
                };
                let collector = DataCollector::new(collector_config);
                let collector = Arc::new(collector);

                // Create overlay widget
                self.overlay_widget = Some(OverlayWidget::new(settings.clone(), collector));
            }
        }
    }

    /// Save settings and update overlay
    fn save_settings(&mut self) {
        // Save settings via config window
        self.config_window.save_settings();

        // Get updated settings
        let new_settings = self.config_window.settings();

        // Update overlay if visible
        if self.overlay_visible {
            if let Some(ref mut overlay) = self.overlay_widget {
                overlay.update_settings(new_settings.clone());
            }
        }
    }
}

impl eframe::App for SimTraceApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Show config window (always visible, small)
        egui::Window::new("⚙️ Config")
            .resizable(true)
            .default_size([400.0, 450.0])
            .anchor(egui::Align2::LEFT_TOP, [10.0, 10.0])
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.heading("SimTrace");
                    ui.separator();

                    // Action buttons
                    ui.horizontal(|ui| {
                        if ui.button("Toggle Overlay").clicked() {
                            self.toggle_overlay();
                        }
                        if ui.button("Save Settings").clicked() {
                            self.save_settings();
                        }
                    });

                    ui.add_space(10.0);
                    ui.separator();

                    // Quick settings
                    ui.label("Quick Settings");
                    let mut settings = self.config_window.settings();
                    ui.horizontal(|ui| {
                        ui.label("Width:");
                        ui.add(egui::Slider::new(
                            &mut settings.overlay.width,
                            300.0..=1200.0,
                        ));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Opacity:");
                        ui.add(egui::Slider::new(&mut settings.overlay.opacity, 0.1..=1.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Time Window:");
                        ui.add(egui::Slider::new(
                            &mut settings.graph.window_seconds,
                            2.0..=30.0,
                        ));
                    });
                    // Apply settings immediately
                    self.config_window.set_settings_mut(settings);

                    // Status
                    ui.add_space(10.0);
                    ui.separator();
                    if let Some(err) = self.config_window.error_message() {
                        ui.label(egui::RichText::new(err).small().color(egui::Color32::RED));
                    }
                    if self.config_window.settings_saved() {
                        ui.label(
                            egui::RichText::new("✓ Saved!")
                                .small()
                                .color(egui::Color32::GREEN),
                        );
                        self.config_window.clear_settings_saved();
                    }
                });
            });

        // Show overlay widget if visible
        if self.overlay_visible {
            if let Some(ref mut overlay) = self.overlay_widget {
                // Poll telemetry
                overlay.collector().poll();

                // Update telemetry values
                overlay.update_telemetry();

                // Show the overlay (separate borderless window)
                overlay.show(ctx);
            }
        }
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

/// Configure egui settings
fn configure_egui(ctx: &egui::Context) {
    let style = (*ctx.style()).clone();
    ctx.set_style(style);
}
