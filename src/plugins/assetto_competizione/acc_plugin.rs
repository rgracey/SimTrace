//! Real ACC plugin using Windows shared memory

use std::f32::consts::PI;

use anyhow::Result;
use tracing::{info, warn};

use crate::core::{SessionInfo, TelemetryData, VehicleTelemetry};
use crate::plugins::{GameConfig, GamePlugin};

use super::mapping::{decode_wstring, status};
use super::shared_memory::AccSharedMemory;

pub struct AccPlugin {
    mem: Option<AccSharedMemory>,
}

impl AccPlugin {
    pub fn new() -> Self {
        Self { mem: None }
    }
}

impl Default for AccPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl GamePlugin for AccPlugin {
    fn name(&self) -> &str {
        "Assetto Corsa Competizione"
    }

    fn connect(&mut self) -> Result<()> {
        match AccSharedMemory::open() {
            Ok(mem) => {
                info!("Connected to ACC shared memory");
                self.mem = Some(mem);
                Ok(())
            }
            Err(e) => {
                warn!("ACC shared memory unavailable: {}", e);
                Err(e)
            }
        }
    }

    fn disconnect(&mut self) {
        self.mem = None; // Drop cleans up handles
        info!("Disconnected from ACC shared memory");
    }

    fn is_connected(&self) -> bool {
        self.mem.is_some()
    }

    fn is_available(&self) -> bool {
        AccSharedMemory::is_available()
    }

    fn read_telemetry(&mut self) -> Result<Option<TelemetryData>> {
        let mem = match &self.mem {
            Some(m) => m,
            None => return Ok(None),
        };

        // Safety: pointer is valid while mem is alive
        let physics = unsafe { mem.physics() };
        let graphics = unsafe { mem.graphics() };

        // Only emit data when the game session is live or replaying
        if graphics.status != status::LIVE && graphics.status != status::REPLAY {
            return Ok(None);
        }

        // ACC steer_angle is the steering wheel angle in radians
        let steering_angle = physics.steer_angle * (180.0 / PI);

        // Gear encoding: 0=R, 1=N, 2=1st … → our model: -1=R, 0=N, 1=1st …
        let gear = physics.gear - 1;

        let vehicle = VehicleTelemetry {
            throttle: physics.gas.clamp(0.0, 1.0),
            brake: physics.brake.clamp(0.0, 1.0),
            clutch: physics.clutch.clamp(0.0, 1.0),
            steering_angle,
            speed: physics.speed_kmh / 3.6, // km/h → m/s
            gear,
            rpm: physics.rpms as f32,
            abs_active: physics.abs > 0.01,
            tc_active: physics.tc > 0.01,
            track_position: graphics.normalized_car_position,
        };

        let static_info = unsafe { mem.static_info() };
        let track_name = decode_wstring(&static_info.track);
        let car_name = decode_wstring(&static_info.car_model);

        let session = SessionInfo {
            session_number: graphics.session as u32,
            session_type: String::new(),
            session_time: graphics.session_time_left,
            track_length: 0.0, // not exposed in SPageFileStatic
            track_name,
            car_name,
            completed_laps: graphics.completed_laps as u32,
            current_lap_time_ms: graphics.i_current_time,
            last_lap_time_ms: graphics.i_last_time,
        };

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(Some(TelemetryData {
            timestamp,
            vehicle,
            session: Some(session),
        }))
    }

    fn get_config(&self) -> GameConfig {
        GameConfig {
            max_steering_angle: 450.0, // typical ACC steering lock (900° total)
            pedal_deadzone: 0.01,
            abs_threshold: 0.01,
        }
    }
}

/// Read car and track names from static shared memory (best-effort).
/// Returns ("", "") if ACC is not running.
#[allow(dead_code)]
pub fn read_session_names() -> (String, String) {
    match AccSharedMemory::open() {
        Ok(mem) => {
            let s = unsafe { mem.static_info() };
            let car = decode_wstring(&s.car_model);
            let track = decode_wstring(&s.track);
            (car, track)
        }
        Err(_) => (String::new(), String::new()),
    }
}
