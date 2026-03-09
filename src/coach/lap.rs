//! Lap recording and boundary detection.
#![allow(dead_code)]
//!
//! `LapRecorder` consumes `TelemetryPoint`s one at a time and emits
//! a completed `LapData` each time the driver crosses the start/finish line.

use crate::core::{SessionInfo, TelemetryPoint};

/// A single telemetry sample distilled for coach use.
#[derive(Debug, Clone)]
pub struct LapSample {
    /// Normalised track position 0.0–1.0.
    pub track_pos: f32,
    pub speed_kph: f32,
    pub throttle: f32,
    pub brake: f32,
    /// Degrees — negative = left, positive = right.
    pub steering_angle: f32,
    pub gear: i32,
    pub rpm: f32,
    pub abs_active: bool,
    pub tc_active: bool,
    /// Milliseconds elapsed since the start of this lap.
    pub lap_elapsed_ms: u32,
    /// World-space X coordinate in metres. 0.0 means unavailable.
    pub world_x: f32,
    /// World-space Z coordinate in metres. 0.0 means unavailable.
    pub world_z: f32,
}

impl LapSample {
    pub fn from_point(point: &TelemetryPoint, lap_start: std::time::Instant) -> Self {
        Self {
            track_pos: point.telemetry.track_position,
            speed_kph: point.telemetry.speed * 3.6,
            throttle: point.telemetry.throttle,
            brake: point.telemetry.brake,
            steering_angle: point.telemetry.steering_angle,
            gear: point.telemetry.gear,
            rpm: point.telemetry.rpm,
            abs_active: point.abs_active,
            tc_active: point.telemetry.tc_active,
            lap_elapsed_ms: lap_start.elapsed().as_millis() as u32,
            world_x: point.telemetry.world_x,
            world_z: point.telemetry.world_z,
        }
    }
}

/// All samples from one fully completed lap.
pub struct LapData {
    pub samples: Vec<LapSample>,
    pub lap_number: u32,
    /// `Some` when the game provides lap timing.
    pub lap_time_ms: Option<u32>,
    pub track_name: String,
    pub car_name: String,
    pub track_length_m: f32,
}

/// Collects telemetry samples and fires a `LapData` on each lap completion.
///
/// Lap boundaries are detected two ways, in priority order:
/// 1. `SessionInfo.completed_laps` increments (authoritative, game-provided).
/// 2. `track_position` wraps from > 0.85 to < 0.15 (fallback).
pub struct LapRecorder {
    current_samples: Vec<LapSample>,
    lap_number: u32,
    lap_start: std::time::Instant,
    last_track_pos: f32,
    last_completed_laps: u32,
    track_name: String,
    car_name: String,
    track_length_m: f32,
}

impl LapRecorder {
    pub fn new() -> Self {
        Self {
            current_samples: Vec::with_capacity(8000),
            lap_number: 0,
            lap_start: std::time::Instant::now(),
            last_track_pos: -1.0,
            last_completed_laps: 0,
            track_name: String::new(),
            car_name: String::new(),
            track_length_m: 0.0,
        }
    }

    /// Feed the next telemetry point. Returns `Some(LapData)` when a lap completes.
    pub fn push(
        &mut self,
        point: &TelemetryPoint,
        session: Option<&SessionInfo>,
    ) -> Option<LapData> {
        // Update session metadata when available.
        if let Some(s) = session {
            if !s.track_name.is_empty() {
                self.track_name = s.track_name.clone();
            }
            if !s.car_name.is_empty() {
                self.car_name = s.car_name.clone();
            }
            if s.track_length > 0.0 {
                self.track_length_m = s.track_length;
            }
        }

        let track_pos = point.telemetry.track_position;
        let sample = LapSample::from_point(point, self.lap_start);
        self.current_samples.push(sample);

        // Determine if a lap boundary was crossed.
        let via_session = session
            .map(|s| s.completed_laps > self.last_completed_laps)
            .unwrap_or(false);
        let via_position = self.last_track_pos > 0.85 && track_pos < 0.15;
        let lap_complete = (via_session || via_position) && self.current_samples.len() > 60;

        if via_session {
            if let Some(s) = session {
                self.last_completed_laps = s.completed_laps;
            }
        }
        self.last_track_pos = track_pos;

        if lap_complete {
            let lap_time_ms = session.and_then(|s| {
                if s.last_lap_time_ms > 0 {
                    Some(s.last_lap_time_ms as u32)
                } else {
                    None
                }
            });

            self.lap_number += 1;
            self.lap_start = std::time::Instant::now();
            let samples = std::mem::replace(&mut self.current_samples, Vec::with_capacity(8000));

            return Some(LapData {
                samples,
                lap_number: self.lap_number,
                lap_time_ms,
                track_name: self.track_name.clone(),
                car_name: self.car_name.clone(),
                track_length_m: self.track_length_m,
            });
        }

        None
    }

    /// View of the samples collected so far this lap (for real-time analysis).
    pub fn current_samples(&self) -> &[LapSample] {
        &self.current_samples
    }
}

impl Default for LapRecorder {
    fn default() -> Self {
        Self::new()
    }
}
