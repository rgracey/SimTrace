//! Mock plugin — generates simulated telemetry for development/testing

use anyhow::Result;

use crate::core::{SessionInfo, TelemetryData, VehicleTelemetry};
use crate::plugins::{GameConfig, GamePlugin};

/// Simulated lap time for the mock track (seconds).
const MOCK_LAP_SECS: f32 = 90.0;

/// Simulated telemetry plugin (always available, no game required)
pub struct MockPlugin {
    connected: bool,
    simulation_time: f32,
    last_update: Option<std::time::Instant>,
    completed_laps: u32,
}

impl MockPlugin {
    pub fn new() -> Self {
        Self {
            connected: false,
            simulation_time: 0.0,
            last_update: None,
            completed_laps: 0,
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

        let t = self.simulation_time;

        // Vary corner period slightly so it never feels like a perfect loop.
        let period = 6.0 + (t * 0.07).sin() * 0.8;
        let phase = (t % period) / period; // 0..1 within each corner cycle

        // ── Phase boundaries ─────────────────────────────────────────────────
        // 0.00 – 0.38  straight, full throttle
        // 0.38 – 0.50  brake zone: hard initial bite, trail off
        // 0.50 – 0.68  apex: brake fully released, max steering
        // 0.68 – 0.88  exit: throttle builds, steering unwinds
        // 0.88 – 1.00  full throttle to next straight

        // Smooth-step helper: ease-in-out curve for natural transitions.
        let smooth = |x: f32| x * x * (3.0 - 2.0 * x);

        // ── Brake ────────────────────────────────────────────────────────────
        let brake = if phase < 0.38 {
            0.0
        } else if phase < 0.44 {
            // Hard initial application — ramps to peak quickly.
            let f = (phase - 0.38) / 0.06;
            smooth(f) * (0.88 + (t * 1.3).sin() * 0.08)
        } else if phase < 0.68 {
            // Trail braking: progressive release through the corner.
            let f = (phase - 0.44) / 0.24;
            (1.0 - smooth(f)) * (0.88 + (t * 1.3).sin() * 0.08)
        } else {
            0.0
        };

        // ── Throttle ─────────────────────────────────────────────────────────
        let throttle = if phase < 0.38 {
            // Straight — full throttle with a small ripple from road bumps.
            0.88 + (t * 4.3).sin() * 0.04 + (t * 2.1).sin() * 0.05
        } else if phase < 0.46 {
            // Lift at the brake point — quick but not instant.
            let f = (phase - 0.38) / 0.08;
            (1.0 - smooth(f.min(1.0))) * 0.9
        } else if phase < 0.66 {
            // Coasting through corner.
            0.0
        } else if phase < 0.88 {
            // Progressive throttle application on exit.
            let f = (phase - 0.66) / 0.22;
            smooth(f) * (0.9 + (t * 0.9).sin() * 0.06)
        } else {
            0.88 + (t * 4.3).sin() * 0.04
        };

        // ── Steering ─────────────────────────────────────────────────────────
        let max_steer = 340.0 + (t * 0.11).sin() * 40.0; // vary corner tightness
        let steer_base = if phase < 0.38 {
            (t * 0.4).sin() * 15.0 // gentle track-following on straight
        } else if phase < 0.68 {
            let f = (phase - 0.38) / 0.30;
            let peak = if f < 0.5 { smooth(f * 2.0) } else { 1.0 };
            peak * max_steer
        } else if phase < 0.90 {
            let f = (phase - 0.68) / 0.22;
            (1.0 - smooth(f)) * max_steer
        } else {
            0.0
        };
        // Erratic corrections while braking — makes the phase plot interesting.
        let wobble = (t * 3.7).sin() * 40.0 + (t * 7.1).sin() * 20.0;
        let steering_angle = steer_base + wobble * brake;

        // ── Derived channels ─────────────────────────────────────────────────
        let speed = (180.0 - brake * 130.0 - (1.0 - throttle) * 20.0).max(40.0);
        let abs_active = brake > 0.72 && phase < 0.47;
        let rpm = 4000.0 + throttle * 6000.0 + (t * 8.0).sin() * 200.0;
        let gear = match speed as u32 {
            0..=60 => 2,
            61..=100 => 3,
            101..=140 => 4,
            _ => 5,
        };

        // Clutch blip on downshifts (brief pulse as brake ramps up).
        let clutch = if (0.38..0.46).contains(&phase) {
            let f = (phase - 0.38) / 0.08;
            ((t * 18.0).sin() * 0.5 + 0.5) * (1.0 - f) * 0.9
        } else {
            0.0
        };

        VehicleTelemetry {
            throttle: throttle.clamp(0.0, 1.0),
            brake: brake.clamp(0.0, 1.0),
            clutch: clutch.clamp(0.0, 1.0),
            steering_angle,
            speed: speed / 3.6,
            gear,
            rpm,
            abs_active,
            tc_active: false,
            track_position: (t / MOCK_LAP_SECS).fract(),
            world_x: 0.0,
            world_z: 0.0,
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

        let lap_elapsed = self.simulation_time % MOCK_LAP_SECS;
        let new_completed = (self.simulation_time / MOCK_LAP_SECS) as u32;
        let last_lap_time_ms = if new_completed > self.completed_laps {
            self.completed_laps = new_completed;
            (MOCK_LAP_SECS * 1000.0) as i32
        } else {
            -1
        };

        let session = SessionInfo {
            session_number: 1,
            session_type: "Practice".to_string(),
            session_time: self.simulation_time,
            track_length: 5000.0,
            track_name: "Mock Circuit".to_string(),
            car_name: "Mock GT3".to_string(),
            completed_laps: self.completed_laps,
            current_lap_time_ms: (lap_elapsed * 1000.0) as i32,
            last_lap_time_ms,
        };

        Ok(Some(TelemetryData {
            timestamp: (self.simulation_time * 1000.0) as u64,
            vehicle: telemetry,
            session: Some(session),
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
