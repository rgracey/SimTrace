//! Common telemetry model - normalized data structure for all games
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Main telemetry data structure returned by game plugins
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryData {
    /// Timestamp from the game (if available)
    pub timestamp: u64,
    /// Vehicle telemetry data
    pub vehicle: VehicleTelemetry,
    /// Session information (optional, for future features)
    pub session: Option<SessionInfo>,
}

/// Normalized vehicle telemetry
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VehicleTelemetry {
    /// Throttle position: 0.0 (no throttle) to 1.0 (full throttle)
    pub throttle: f32,
    /// Brake position: 0.0 (no brake) to 1.0 (full brake)
    pub brake: f32,
    /// Clutch position: 0.0 (no clutch) to 1.0 (full clutch)
    pub clutch: f32,
    /// Steering angle in degrees (negative = left, positive = right)
    pub steering_angle: f32,
    /// Vehicle speed in m/s
    pub speed: f32,
    /// Current gear (1-7, 0 = neutral, -1 = reverse)
    pub gear: i32,
    /// Engine RPM
    pub rpm: f32,
    /// ABS is currently active
    pub abs_active: bool,
    /// Traction control is currently active
    pub tc_active: bool,
    /// Track position: 0.0 to 1.0 along the track
    pub track_position: f32,
}

/// Session information (optional)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionInfo {
    /// Session number
    pub session_number: u32,
    /// Session type (practice, qualifying, race, etc.)
    pub session_type: String,
    /// Total session time in seconds
    pub session_time: f32,
    /// Track length in meters
    pub track_length: f32,
    /// Track name
    pub track_name: String,
    /// Car name
    pub car_name: String,
}

/// A telemetry point stored in the buffer
#[derive(Debug, Clone)]
pub struct TelemetryPoint {
    /// When this point was captured
    pub captured_at: Instant,
    /// Telemetry data
    pub telemetry: VehicleTelemetry,
    /// ABS state at capture time (persisted for coloring)
    pub abs_active: bool,
}

impl TelemetryPoint {
    pub fn new(telemetry: VehicleTelemetry, abs_active: bool) -> Self {
        Self {
            captured_at: Instant::now(),
            telemetry,
            abs_active,
        }
    }
}

impl VehicleTelemetry {
    /// Create a new vehicle telemetry with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any pedal is being pressed
    pub fn has_pedal_input(&self) -> bool {
        self.throttle > 0.01 || self.brake > 0.01 || self.clutch > 0.01
    }

    /// Get the maximum pedal input (throttle or brake)
    pub fn max_pedal(&self) -> f32 {
        self.throttle.max(self.brake)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pedal_input_detection() {
        let telemetry = VehicleTelemetry {
            throttle: 0.5,
            brake: 0.0,
            ..Default::default()
        };
        assert!(telemetry.has_pedal_input());

        let telemetry = VehicleTelemetry::default();
        assert!(!telemetry.has_pedal_input());
    }

    #[test]
    fn test_max_pedal() {
        let telemetry = VehicleTelemetry {
            throttle: 0.3,
            brake: 0.7,
            ..Default::default()
        };
        assert_eq!(telemetry.max_pedal(), 0.7);
    }
}
