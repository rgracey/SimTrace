//! Configuration window for initial setup

use eframe::egui;

use crate::config::AppSettings;

/// Get config file path
fn get_config_path() -> Option<std::path::PathBuf> {
    dirs::config_dir()
        .map(|p| p.join("simtrace").join("settings.toml"))
        .or_else(|| dirs::home_dir().map(|p| p.join(".simtrace").join("settings.toml")))
}

/// Configuration window shown on first launch
pub struct ConfigWindow {
    /// Current settings being configured
    settings: AppSettings,
    /// Whether settings have been saved
    settings_saved: bool,
    /// Error message if any
    error_message: Option<String>,
}

impl ConfigWindow {
    /// Create a new configuration window
    pub fn new() -> Self {
        Self {
            settings: AppSettings::default(),
            settings_saved: false,
            error_message: None,
        }
    }

    /// Set settings from loaded config
    pub fn set_settings(&mut self, settings: AppSettings) {
        self.settings = settings;
    }

    /// Get the configured settings (clone)
    pub fn settings(&self) -> AppSettings {
        self.settings.clone()
    }

    /// Set settings (for quick settings updates)
    pub fn set_settings_mut(&mut self, settings: AppSettings) {
        self.settings = settings;
    }

    /// Get error message
    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    /// Set error message
    pub fn set_error_message(&mut self, msg: Option<String>) {
        self.error_message = msg;
    }

    /// Check if settings were just saved
    pub fn settings_saved(&self) -> bool {
        self.settings_saved
    }

    /// Clear settings saved flag
    pub fn clear_settings_saved(&mut self) {
        self.settings_saved = false;
    }

    /// Save settings to file
    pub fn save_settings(&mut self) {
        let config_path: Option<std::path::PathBuf> = dirs::config_dir()
            .map(|p| p.join("simtrace").join("settings.toml"))
            .or_else(|| dirs::home_dir().map(|p| p.join(".simtrace").join("settings.toml")));

        match config_path {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        self.error_message =
                            Some(format!("Failed to create config directory: {}", e));
                        return;
                    }
                }

                match self.settings.save(&path) {
                    Ok(()) => {
                        self.settings_saved = true;
                        tracing::info!("Settings saved to {:?}", path);
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to save settings: {}", e));
                    }
                }
            }
            None => {
                self.error_message = Some("Could not determine config directory".to_string());
            }
        }
    }

    /// Show the configuration UI
    pub fn show(&mut self, ctx: &egui::Context) {
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
                        if ui.button("Save Settings").clicked() {
                            self.save_settings();
                        }
                        if ui.button("Reset").clicked() {
                            self.settings = AppSettings::default();
                            self.error_message = None;
                        }
                    });

                    ui.add_space(10.0);
                    ui.separator();

                    // Settings sections
                    ui.label("Quick Settings");

                    self.show_overlay_settings(ui);

                    ui.add_space(10.0);
                    ui.separator();

                    self.show_graph_settings(ui);

                    ui.add_space(10.0);
                    ui.separator();

                    // Status messages
                    if let Some(ref error) = self.error_message {
                        ui.label(egui::RichText::new(error).small().color(egui::Color32::RED));
                    }

                    if self.settings_saved {
                        ui.label(
                            egui::RichText::new("✓ Saved!")
                                .small()
                                .color(egui::Color32::GREEN),
                        );
                        self.settings_saved = false;
                    }
                });
            });
    }

    /// Show basic settings
    fn show_basic_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Update Rate:");
            ui.add(
                egui::Slider::new(&mut self.settings.collector.update_rate_hz, 30..=120)
                    .suffix(" Hz"),
            );
        });

        ui.horizontal(|ui| {
            ui.label("Plugin:");
            // For now, just show the default
            ui.label(&self.settings.collector.plugin);
            ui.label("(Will be configured later)");
        });

        ui.horizontal(|ui| {
            ui.label("Buffer Window:");
            if let Some(ref mut secs) = self.settings.collector.buffer_window_secs {
                ui.add(egui::Slider::new(secs, 5..=30).suffix(" s"));
            }
        });
    }

    /// Show overlay settings
    fn show_overlay_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Width:");
            ui.add(
                egui::Slider::new(&mut self.settings.overlay.width, 300.0..=1200.0).suffix(" px"),
            );
        });

        ui.horizontal(|ui| {
            ui.label("Height:");
            ui.add(
                egui::Slider::new(&mut self.settings.overlay.height, 200.0..=800.0).suffix(" px"),
            );
        });

        ui.horizontal(|ui| {
            ui.label("Opacity:");
            ui.add(egui::Slider::new(&mut self.settings.overlay.opacity, 0.1..=1.0).suffix(" %"));
        });

        ui.horizontal(|ui| {
            ui.label("Position X:");
            ui.add(egui::Slider::new(
                &mut self.settings.overlay.position_x,
                0.0..=1920.0,
            ));
        });

        ui.horizontal(|ui| {
            ui.label("Position Y:");
            ui.add(egui::Slider::new(
                &mut self.settings.overlay.position_y,
                0.0..=1080.0,
            ));
        });
    }

    /// Show graph settings
    fn show_graph_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Time Window:");
            ui.add(
                egui::Slider::new(&mut self.settings.graph.window_seconds, 2.0..=30.0).suffix(" s"),
            );
        });

        ui.horizontal(|ui| {
            ui.label("Line Width:");
            ui.add(egui::Slider::new(&mut self.settings.graph.line_width, 1.0..=5.0).step_by(0.5));
        });

        ui.checkbox(&mut self.settings.graph.show_grid, "Show Grid");
        ui.checkbox(&mut self.settings.graph.show_legend, "Show Legend");
    }

    /// Show color settings
    fn show_color_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Throttle:");
            if ui.button(&self.settings.colors.throttle).clicked() {
                // TODO: Implement color picker
            }
        });

        ui.horizontal(|ui| {
            ui.label("Brake:");
            if ui.button(&self.settings.colors.brake).clicked() {
                // TODO: Implement color picker
            }
        });

        ui.horizontal(|ui| {
            ui.label("ABS Active:");
            if ui.button(&self.settings.colors.abs_active).clicked() {
                // TODO: Implement color picker
            }
        });

        ui.horizontal(|ui| {
            ui.label("Background:");
            if ui.button(&self.settings.colors.background).clicked() {
                // TODO: Implement color picker
            }
        });
    }
}

impl Default for ConfigWindow {
    fn default() -> Self {
        Self::new()
    }
}
