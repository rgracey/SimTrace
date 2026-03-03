//! Game plugin trait definition

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

/// Helper function to create a plugin by name
pub fn create_plugin(name: &str) -> Option<Box<dyn GamePlugin>> {
    match name.to_lowercase().as_str() {
        "assetto corsa competizione" | "acc" => {
            #[cfg(windows)]
            {
                Some(Box::new(
                    crate::plugins::assetto_competizione::AccPlugin::new(),
                ))
            }
            #[cfg(not(windows))]
            {
                // On non-Windows, ACC is not available
                None
            }
        }
        _ => None,
    }
}
