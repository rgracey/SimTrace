//! Application settings — serialized to/from TOML on disk.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level settings container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub collector: CollectorConfig,
    pub graph: GraphSettings,
    pub colors: ColorScheme,
    pub overlay: OverlaySettings,
}

/// Which game plugin is active.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorConfig {
    pub plugin: String,
}

/// Graph visualization settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSettings {
    /// Seconds of history shown in the trace graph.
    pub window_seconds: f64,
    pub show_grid: bool,
    pub show_legend: bool,
    pub line_width: f32,
    /// Target repaint rate for the overlay.
    #[serde(default = "default_overlay_fps")]
    pub overlay_fps: u32,
    /// Display speed in mph instead of kph.
    #[serde(default)]
    pub speed_mph: bool,
    #[serde(default = "default_true")]
    pub show_throttle: bool,
    #[serde(default = "default_true")]
    pub show_brake: bool,
    #[serde(default = "default_true")]
    pub show_abs: bool,
    #[serde(default = "default_true")]
    pub show_clutch: bool,
}

fn default_true() -> bool {
    true
}

fn default_overlay_fps() -> u32 {
    60
}

/// Trace and bar colours (hex strings, e.g. `"#FF0000"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorScheme {
    pub throttle: String,
    pub brake: String,
    pub abs_active: String,
    #[serde(default = "default_clutch_color")]
    pub clutch: String,
    pub background: String,
    pub grid: String,
    pub text: String,
}

fn default_clutch_color() -> String {
    "#AA44FF".to_string()
}

/// Overlay window geometry and appearance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlaySettings {
    pub width: f32,
    pub height: f32,
    pub position_x: f32,
    pub position_y: f32,
    /// Overall transparency (0.0 = invisible, 1.0 = opaque).
    pub opacity: f32,
    pub pinned: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            collector: CollectorConfig {
                plugin: "mock".to_string(),
            },
            graph: GraphSettings {
                window_seconds: 10.0,
                show_grid: true,
                show_legend: true,
                line_width: 2.0,
                overlay_fps: 60,
                speed_mph: false,
                show_throttle: true,
                show_brake: true,
                show_abs: true,
                show_clutch: true,
            },
            colors: ColorScheme {
                throttle: "#00FF00".to_string(),
                brake: "#FF0000".to_string(),
                abs_active: "#FFA500".to_string(),
                clutch: "#AA44FF".to_string(),
                background: "#1A1A1A".to_string(),
                grid: "#333333".to_string(),
                text: "#FFFFFF".to_string(),
            },
            overlay: OverlaySettings {
                width: 600.0,
                height: 400.0,
                position_x: 100.0,
                position_y: 100.0,
                opacity: 1.0,
                pinned: false,
            },
        }
    }
}

impl AppSettings {
    /// Returns the platform-appropriate directory for config/log files.
    pub fn config_dir() -> Option<PathBuf> {
        dirs::config_dir()
            .map(|p| p.join("simtrace"))
            .or_else(|| dirs::home_dir().map(|p| p.join(".simtrace")))
    }

    /// Returns the platform-appropriate path for the settings file.
    fn config_path() -> Option<PathBuf> {
        dirs::config_dir()
            .map(|p| p.join("simtrace").join("settings.toml"))
            .or_else(|| dirs::home_dir().map(|p| p.join(".simtrace").join("settings.toml")))
    }

    /// Loads settings from the default config path, or returns defaults if the
    /// file doesn't exist or can't be parsed.
    pub fn load_or_default() -> Self {
        Self::config_path()
            .and_then(|p| Self::load(&p).ok())
            .unwrap_or_default()
    }

    /// Saves settings to the default config path, creating directories as needed.
    pub fn save_to_config_path(&self) -> Result<(), anyhow::Error> {
        let path =
            Self::config_path().ok_or_else(|| anyhow::anyhow!("No config directory found"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        self.save(&path)
    }

    /// Loads settings from an arbitrary file path.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, anyhow::Error> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    /// Saves settings to an arbitrary file path.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), anyhow::Error> {
        Ok(std::fs::write(path, toml::to_string_pretty(self)?)?)
    }

    /// Parses a `#RRGGBB` hex string into an egui `Color32`.
    /// Returns white on any parse error.
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
        let s = AppSettings::default();
        assert_eq!(s.collector.plugin, "mock");
        assert_eq!(s.overlay.opacity, 1.0);
        assert_eq!(s.graph.window_seconds, 10.0);
    }

    #[test]
    fn test_settings_round_trip() {
        let original = AppSettings::default();
        let toml_str = toml::to_string_pretty(&original).unwrap();
        let restored: AppSettings = toml::from_str(&toml_str).unwrap();
        assert_eq!(restored.collector.plugin, original.collector.plugin);
        assert_eq!(restored.overlay.opacity, original.overlay.opacity);
        assert_eq!(restored.graph.window_seconds, original.graph.window_seconds);
        assert_eq!(restored.colors.throttle, original.colors.throttle);
    }

    #[test]
    fn test_settings_missing_optional_fields_use_defaults() {
        // A minimal TOML with only required fields — optional fields should default correctly.
        let toml_str = r##"
            [collector]
            plugin = "mock"

            [graph]
            window_seconds = 10.0
            show_grid = true
            show_legend = true
            line_width = 2.0

            [colors]
            throttle = "#00FF00"
            brake = "#FF0000"
            abs_active = "#FFA500"
            background = "#1A1A1A"
            grid = "#333333"
            text = "#FFFFFF"

            [overlay]
            width = 600.0
            height = 400.0
            position_x = 100.0
            position_y = 100.0
            opacity = 1.0
            pinned = false
        "##;
        let s: AppSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(s.graph.overlay_fps, default_overlay_fps());
        assert_eq!(s.colors.clutch, default_clutch_color());
        assert!(!s.graph.speed_mph);
    }

    #[test]
    fn test_parse_color_valid() {
        assert_eq!(
            AppSettings::parse_color("#FF0000"),
            egui::Color32::from_rgb(255, 0, 0)
        );
        assert_eq!(
            AppSettings::parse_color("#00ff00"),
            egui::Color32::from_rgb(0, 255, 0)
        );
        assert_eq!(
            AppSettings::parse_color("1A2B3C"),
            egui::Color32::from_rgb(0x1A, 0x2B, 0x3C)
        );
    }

    #[test]
    fn test_parse_color_invalid_returns_white() {
        assert_eq!(AppSettings::parse_color(""), egui::Color32::WHITE);
        assert_eq!(AppSettings::parse_color("#FFF"), egui::Color32::WHITE);
        assert_eq!(
            AppSettings::parse_color("not-a-color"),
            egui::Color32::WHITE
        );
    }
}
