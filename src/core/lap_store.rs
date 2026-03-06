//! Lap boundary detection and per-lap telemetry storage.

use crate::core::TelemetryPoint;
use std::time::Instant;

/// A single telemetry sample stored within a lap, keyed by track position.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct LapPoint {
    /// Track position: 0.0 = start/finish line, 1.0 = just before it.
    pub track_position: f32,
    pub throttle: f32,
    pub brake: f32,
    pub speed: f32,
    pub abs_active: bool,
    /// Milliseconds elapsed since the first point of this lap.
    pub elapsed_ms: f32,
}

/// Detects lap crossings and maintains a reference lap for comparison.
#[derive(Clone, Default)]
pub struct LapStore {
    /// The most recently completed full lap, sorted by `track_position`.
    pub reference_lap: Option<Vec<LapPoint>>,
    current_lap: Vec<LapPoint>,
    lap_start_at: Option<Instant>,
    last_track_pos: Option<f32>,
    /// Deduplication: skip if this point was already processed.
    last_captured_at: Option<Instant>,
}

impl LapStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push the latest telemetry point.
    ///
    /// When `track_position` crosses the start/finish line (wraps from near
    /// 1.0 back to near 0.0), the completed lap is saved as the reference.
    pub fn push(&mut self, pt: &TelemetryPoint) {
        // `buffer.latest()` returns the same point every frame until a new one
        // arrives, so we skip duplicates by comparing captured_at instants.
        if self.last_captured_at == Some(pt.captured_at) {
            return;
        }
        self.last_captured_at = Some(pt.captured_at);

        let pos = pt.telemetry.track_position;

        let crossed = self
            .last_track_pos
            .map(|last| last > 0.85 && pos < 0.15)
            .unwrap_or(false);

        if crossed && self.current_lap.len() > 20 {
            // Completed a valid lap — promote to reference.
            let mut completed = std::mem::take(&mut self.current_lap);
            // Sort by track_position so the comparison panel can interpolate.
            completed.sort_by(|a, b| {
                a.track_position
                    .partial_cmp(&b.track_position)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            self.reference_lap = Some(completed);
            self.lap_start_at = None;
        }

        // Begin timing the new lap on its first point.
        if self.current_lap.is_empty() {
            self.lap_start_at = Some(pt.captured_at);
        }

        if let Some(start) = self.lap_start_at {
            self.current_lap.push(LapPoint {
                track_position: pos,
                throttle: pt.telemetry.throttle,
                brake: pt.telemetry.brake,
                speed: pt.telemetry.speed,
                abs_active: pt.abs_active,
                elapsed_ms: pt.captured_at.duration_since(start).as_secs_f32() * 1000.0,
            });
        }

        self.last_track_pos = Some(pos);
    }

    /// The telemetry points for the lap currently in progress, in push order.
    pub fn current_lap(&self) -> &[LapPoint] {
        &self.current_lap
    }

    /// Promote the current partial lap to the reference immediately.
    pub fn set_current_as_reference(&mut self) {
        if !self.current_lap.is_empty() {
            let mut snap = self.current_lap.clone();
            snap.sort_by(|a, b| {
                a.track_position
                    .partial_cmp(&b.track_position)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            self.reference_lap = Some(snap);
        }
    }

    pub fn clear_reference(&mut self) {
        self.reference_lap = None;
    }

    /// Reset all accumulated data (call after a plugin change).
    pub fn clear(&mut self) {
        self.current_lap.clear();
        self.lap_start_at = None;
        self.last_track_pos = None;
        self.last_captured_at = None;
    }
}
