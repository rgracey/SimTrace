//! Mock plugin — generates simulated telemetry for development/testing

use anyhow::Result;

use crate::core::{TelemetryData, VehicleTelemetry};
use crate::plugins::{GameConfig, GamePlugin};

/// Simulated telemetry plugin (always available, no game required)
pub struct MockPlugin {
    connected: bool,
    simulation_time: f32,
    last_update: Option<std::time::Instant>,
}

impl MockPlugin {
    pub fn new() -> Self {
        Self {
            connected: false,
            simulation_time: 0.0,
            last_update: None,
        }
    }

    fn generate_telemetry(&mut self) -> VehicleTelemetry {
        let elapsed = if let Some(last) = self.last_update {
            std::time::Instant::now().duration_since(last).as_secs_f32()
        } else {
            0.0
        };
        self.last_update = Some(std::time::Instant::now());
        self.simulation_time += elapsed;

        // Simulate throttle/brake pattern (like a chicane)
        let cycle = (self.simulation_time * 0.5).sin();
        let throttle = if cycle > 0.0 { cycle } else { 0.0 };
        let brake = if cycle < -0.3 {
            (-cycle - 0.3).max(0.0)
        } else {
            0.0
        };
        let steering_angle = cycle * 180.0;
        let speed = (100.0 - brake * 80.0).max(20.0);
        let abs_active = brake > 0.6;
        let rpm = 8000.0 + throttle * 2000.0;

        VehicleTelemetry {
            throttle: throttle.clamp(0.0, 1.0),
            brake: brake.clamp(0.0, 1.0),
            clutch: 0.0,
            steering_angle,
            speed: speed / 3.6,
            gear: if speed < 5.0 { 1 } else { 3 },
            rpm,
            abs_active,
            tc_active: false,
            track_position: (self.simulation_time * 0.01).fract(),
        }
    }
}

impl Default for MockPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl GamePlugin for MockPlugin {
    fn name(&self) -> &str {
        "Mock"
    }

    fn connect(&mut self) -> Result<()> {
        self.connected = true;
        self.simulation_time = 0.0;
        self.last_update = Some(std::time::Instant::now());
        Ok(())
    }

    fn disconnect(&mut self) {
        self.connected = false;
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn is_available(&self) -> bool {
        true
    }

    fn read_telemetry(&mut self) -> Result<Option<TelemetryData>> {
        if !self.connected {
            return Ok(None);
        }
        let telemetry = self.generate_telemetry();
        Ok(Some(TelemetryData {
            timestamp: (self.simulation_time * 1000.0) as u64,
            vehicle: telemetry,
            session: None,
        }))
    }

    fn get_config(&self) -> GameConfig {
        GameConfig {
            max_steering_angle: 900.0,
            pedal_deadzone: 0.01,
            abs_threshold: 0.1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_plugin_connection() {
        let mut plugin = MockPlugin::new();
        assert!(!plugin.is_connected());
        plugin.connect().unwrap();
        assert!(plugin.is_connected());
        plugin.disconnect();
        assert!(!plugin.is_connected());
    }

    #[test]
    fn test_mock_telemetry_generation() {
        let mut plugin = MockPlugin::new();
        plugin.connect().unwrap();
        let data = plugin.read_telemetry().unwrap().unwrap();
        assert!((0.0..=1.0).contains(&data.vehicle.throttle));
        assert!((0.0..=1.0).contains(&data.vehicle.brake));
    }
}
