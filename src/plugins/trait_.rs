//! Game plugin trait definition
#![allow(dead_code)]

use anyhow::Result;

use crate::core::TelemetryData;

/// Configuration for a game plugin
#[derive(Debug, Clone, Default)]
pub struct GameConfig {
    /// Maximum steering angle in degrees
    pub max_steering_angle: f32,
    /// Deadzone for pedal inputs
    pub pedal_deadzone: f32,
    /// Threshold for ABS activation detection
    pub abs_threshold: f32,
}

/// Trait that all game plugins must implement
pub trait GamePlugin: Send + Sync {
    /// Get the plugin name
    fn name(&self) -> &str;

    /// Initialize and connect to the game
    fn connect(&mut self) -> Result<()>;

    /// Disconnect from the game
    fn disconnect(&mut self);

    /// Check if currently connected
    fn is_connected(&self) -> bool;

    /// Read telemetry data from the game
    /// Returns None if no data available yet
    fn read_telemetry(&mut self) -> Result<Option<TelemetryData>>;

    /// Get game-specific configuration
    fn get_config(&self) -> GameConfig {
        GameConfig::default()
    }

    /// Check if the game is running/available
    fn is_available(&self) -> bool {
        true
    }
}

/// Returns the static list of `(id, display_name)` pairs available on this platform.
///
/// ACC only ships on Windows; the mock entry is always present.
pub fn plugin_entries() -> &'static [(&'static str, &'static str)] {
    &[
        ("assetto_competizione", "Assetto Corsa Competizione"),
        ("ams2", "Automobilista 2"),
        ("mock", "Mock (Simulated Data)"),
    ]
}

/// Helper function to create a plugin by name
pub fn create_plugin(name: &str) -> Option<Box<dyn GamePlugin>> {
    match name.to_lowercase().as_str() {
        "assetto_competizione" | "assetto corsa competizione" | "acc" => {
            #[cfg(windows)]
            {
                Some(Box::new(
                    crate::plugins::assetto_competizione::AccPlugin::new(),
                ))
            }
            #[cfg(not(windows))]
            {
                None
            }
        }
        "ams2" | "automobilista 2" | "automobilista2" => {
            Some(Box::new(crate::plugins::ams2::Ams2Plugin::new()))
        }
        "mock" | "test" => Some(Box::new(crate::plugins::mock::MockPlugin::new())),
        _ => None,
    }
}
