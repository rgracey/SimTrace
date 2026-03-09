//! iRacing plugin — reads telemetry via the iRacing SDK shared memory API.

use anyhow::Result;

use crate::core::TelemetryData;
use crate::plugins::{GameConfig, GamePlugin};

#[cfg(windows)]
use super::shared_memory::IracingSharedMemory;
#[cfg(windows)]
use crate::core::VehicleTelemetry;
#[cfg(windows)]
use std::f32::consts::PI;
#[cfg(windows)]
use tracing::{info, warn};

pub struct IracingPlugin {
    #[cfg(windows)]
    mem: Option<IracingSharedMemory>,
}

impl IracingPlugin {
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            mem: None,
        }
    }
}

impl Default for IracingPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl GamePlugin for IracingPlugin {
    fn name(&self) -> &str {
        "iRacing"
    }

    fn connect(&mut self) -> Result<()> {
        #[cfg(windows)]
        {
            match IracingSharedMemory::open() {
                Ok(mem) => {
                    info!("Connected to iRacing shared memory");
                    self.mem = Some(mem);
                    Ok(())
                }
                Err(e) => {
                    warn!("iRacing shared memory unavailable: {}", e);
                    Err(e)
                }
            }
        }
        #[cfg(not(windows))]
        {
            Err(anyhow::anyhow!(
                "iRacing plugin requires Windows (shared memory is Windows-only)"
            ))
        }
    }

    fn disconnect(&mut self) {
        #[cfg(windows)]
        {
            self.mem = None;
            info!("Disconnected from iRacing shared memory");
        }
    }

    fn is_connected(&self) -> bool {
        #[cfg(windows)]
        {
            // Verify the session is still active in case iRacing left the session.
            self.mem
                .as_ref()
                .is_some_and(|m| unsafe { m.is_connected() })
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    fn is_available(&self) -> bool {
        #[cfg(windows)]
        {
            IracingSharedMemory::is_available()
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

            if !unsafe { mem.is_connected() } {
                // Session ended — drop the mapping so connect() will rescan var headers.
                self.mem = None;
                return Ok(None);
            }

            let buf = unsafe { mem.current_buf_offset() };

            let throttle = unsafe { mem.throttle(buf) }.clamp(0.0, 1.0);
            let brake = unsafe { mem.brake(buf) }.clamp(0.0, 1.0);
            // iRacing Clutch: 0 = pedal pressed (disengaged), 1 = pedal released (engaged).
            // Invert so that our model convention (1.0 = fully pressed) is respected.
            let clutch = (1.0 - unsafe { mem.clutch_raw(buf) }).clamp(0.0, 1.0);
            // SteeringWheelAngle: radians, positive = CCW (left). Convert to degrees, positive = right.
            let steering_angle = unsafe { mem.steering_wheel_angle_rad(buf) } * -(180.0 / PI);
            let speed = unsafe { mem.speed(buf) };
            let gear = unsafe { mem.gear(buf) };
            let rpm = unsafe { mem.rpm(buf) };
            let abs_active = unsafe { mem.abs_active(buf) };
            let track_position = unsafe { mem.lap_dist_pct(buf) }.clamp(0.0, 1.0);

            let vehicle = VehicleTelemetry {
                throttle,
                brake,
                clutch,
                steering_angle,
                speed,
                gear,
                rpm,
                abs_active,
                tc_active: false,
                track_position,
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
            // iRacing typically uses ±PI radians full lock, but varies by car.
            // 450° is a reasonable UI default (same as ACC/AMS2 convention).
            max_steering_angle: 450.0,
            pedal_deadzone: 0.01,
            abs_threshold: 0.01,
        }
    }
}
