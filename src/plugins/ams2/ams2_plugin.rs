//! AMS2 plugin — reads pedal/steering/speed data via the pCars2 shared memory API.

use anyhow::Result;

use crate::core::TelemetryData;
use crate::plugins::{GameConfig, GamePlugin};

#[cfg(windows)]
use super::shared_memory::Ams2SharedMemory;
#[cfg(windows)]
use crate::core::VehicleTelemetry;
#[cfg(windows)]
use tracing::{info, warn};

/// Game states from the pCars2 shared memory (mGameState field).
#[cfg(windows)]
mod game_state {
    pub const EXITED: u32 = 0;
    pub const FRONT_END: u32 = 1;
}


pub struct Ams2Plugin {
    #[cfg(windows)]
    mem: Option<Ams2SharedMemory>,
}

impl Ams2Plugin {
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            mem: None,
        }
    }
}

impl Default for Ams2Plugin {
    fn default() -> Self {
        Self::new()
    }
}

impl GamePlugin for Ams2Plugin {
    fn name(&self) -> &str {
        "Automobilista 2"
    }

    fn connect(&mut self) -> Result<()> {
        #[cfg(windows)]
        {
            match Ams2SharedMemory::open() {
                Ok(mem) => {
                    info!("Connected to AMS2 shared memory");
                    self.mem = Some(mem);
                    Ok(())
                }
                Err(e) => {
                    warn!("AMS2 shared memory unavailable: {}", e);
                    Err(e)
                }
            }
        }
        #[cfg(not(windows))]
        {
            Err(anyhow::anyhow!(
                "AMS2 plugin requires Windows (shared memory is Windows-only)"
            ))
        }
    }

    fn disconnect(&mut self) {
        #[cfg(windows)]
        {
            self.mem = None;
            info!("Disconnected from AMS2 shared memory");
        }
    }

    fn is_connected(&self) -> bool {
        #[cfg(windows)]
        {
            self.mem.is_some()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    fn is_available(&self) -> bool {
        #[cfg(windows)]
        {
            Ams2SharedMemory::is_available()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    fn read_telemetry(&mut self) -> Result<Option<TelemetryData>> {
        #[cfg(windows)]
        {
            let mem = match &self.mem {
                Some(m) => m,
                None => return Ok(None),
            };

            // Safety: pointer is valid while mem is alive.
            let game_state = unsafe { mem.game_state() };

            // Only emit data when a session is active (not in menus / exited).
            if game_state == game_state::EXITED || game_state == game_state::FRONT_END {
                return Ok(None);
            }

            let throttle = unsafe { mem.unfiltered_throttle() }.clamp(0.0, 1.0);
            let brake = unsafe { mem.unfiltered_brake() }.clamp(0.0, 1.0);
            let clutch = unsafe { mem.unfiltered_clutch() }.clamp(0.0, 1.0);
            // Steering: normalised -1..1 → degrees (450° half-lock matches ACC convention)
            let steering_angle = unsafe { mem.unfiltered_steering() }.clamp(-1.0, 1.0) * 450.0;
            let speed = unsafe { mem.speed() }; // already m/s
            let gear = unsafe { mem.gear() };
            let abs_active = unsafe { mem.anti_lock_active() };

            let vehicle = VehicleTelemetry {
                throttle,
                brake,
                clutch,
                steering_angle,
                speed,
                gear,
                rpm: 0.0, // not read to keep offsets simple
                abs_active,
                tc_active: false,
                track_position: 0.0,
            };

            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            Ok(Some(TelemetryData {
                timestamp,
                vehicle,
                session: None,
            }))
        }
        #[cfg(not(windows))]
        {
            Ok(None)
        }
    }

    fn get_config(&self) -> GameConfig {
        GameConfig {
            max_steering_angle: 450.0,
            pedal_deadzone: 0.01,
            abs_threshold: 0.01,
        }
    }
}
