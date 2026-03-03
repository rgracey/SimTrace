//! Application settings and configuration

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Main application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub app: AppConfig,
    pub collector: CollectorConfig,
    pub graph: GraphSettings,
    pub colors: ColorScheme,
    pub steering_wheel: SteeringWheelSettings,
}

/// Application window configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub fps_limit: u32,
    pub fullscreen: bool,
}

/// Data collector configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorConfig {
    pub update_rate_hz: u32,
    pub plugin: String,
    pub reconnect_delay_ms: u64,
    /// Buffer window in seconds (optional, defaults to 10)
    #[serde(default = "default_buffer_window")]
    pub buffer_window_secs: Option<u64>,
}

fn default_buffer_window() -> Option<u64> {
    Some(10)
}

/// Graph visualization settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSettings {
    pub window_seconds: f64,
    pub show_grid: bool,
    pub show_legend: bool,
    pub line_width: f32,
}

/// Color scheme for visualization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorScheme {
    pub throttle: String,
    pub brake: String,
    pub abs_active: String,
    pub background: String,
    pub grid: String,
    pub text: String,
}

/// Steering wheel visualization settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteeringWheelSettings {
    pub size: u32,
    pub color: String,
    pub center_color: String,
    pub text_color: String,
    pub show_angle: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            app: AppConfig {
                title: "SimTrace".to_string(),
                width: 1280,
                height: 720,
                fps_limit: 60,
                fullscreen: false,
            },
            collector: CollectorConfig {
                update_rate_hz: 60,
                plugin: "assetto_competizione".to_string(),
                reconnect_delay_ms: 1000,
                buffer_window_secs: Some(10),
            },
            graph: GraphSettings {
                window_seconds: 10.0,
                show_grid: true,
                show_legend: true,
                line_width: 2.0,
            },
            colors: ColorScheme {
                throttle: "#00FF00".to_string(),
                brake: "#FF0000".to_string(),
                abs_active: "#FFA500".to_string(),
                background: "#1A1A1A".to_string(),
                grid: "#333333".to_string(),
                text: "#FFFFFF".to_string(),
            },
            steering_wheel: SteeringWheelSettings {
                size: 200,
                color: "#444444".to_string(),
                center_color: "#666666".to_string(),
                text_color: "#FFFFFF".to_string(),
                show_angle: false,
            },
        }
    }
}

impl AppSettings {
    /// Load settings from a file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, anyhow::Error> {
        let content = std::fs::read_to_string(path)?;
        let settings: AppSettings = toml::from_str(&content)?;
        Ok(settings)
    }

    /// Save settings to a file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), anyhow::Error> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Parse a hex color string to egui Color32
    pub fn parse_color(hex: &str) -> egui::Color32 {
        let hex = hex.trim_start_matches('#');
        if hex.len() != 6 {
            return egui::Color32::WHITE;
        }

        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);

        egui::Color32::from_rgb(r, g, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = AppSettings::default();
        assert_eq!(settings.app.width, 1280);
        assert_eq!(settings.app.height, 720);
    }

    #[test]
    fn test_parse_color() {
        let color = AppSettings::parse_color("#FF0000");
        assert_eq!(color, egui::Color32::from_rgb(255, 0, 0));

        let color = AppSettings::parse_color("invalid");
        assert_eq!(color, egui::Color32::WHITE);
    }
}
