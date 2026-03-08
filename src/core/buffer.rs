//! Telemetry buffer - stores telemetry points with a sliding time window
#![allow(dead_code)]

use std::sync::RwLock;
use std::time::Duration;

use crate::core::{SessionInfo, TelemetryPoint, VehicleTelemetry};

/// Buffer storing telemetry points with a configurable time window
pub struct TelemetryBuffer {
    /// Maximum time window to keep
    window_duration: Duration,
    /// Stored telemetry points
    data: RwLock<Vec<TelemetryPoint>>,
    /// Minimum points to keep (prevents empty buffer)
    min_points: usize,
    /// Latest session info received from the active plugin
    session: RwLock<Option<SessionInfo>>,
}

impl TelemetryBuffer {
    /// Create a new buffer with the specified time window
    pub fn new(window_duration: Duration) -> Self {
        Self {
            window_duration,
            data: RwLock::new(Vec::with_capacity(1000)),
            min_points: 10,
            session: RwLock::new(None),
        }
    }

    /// Push a new telemetry point
    pub fn push(&self, telemetry: VehicleTelemetry, abs_active: bool) {
        let point = TelemetryPoint::new(telemetry, abs_active);
        let mut data = self.data.write().unwrap();
        data.push(point);
        self.prune_old_points(&mut data);
    }

    /// Get all points within the current time window
    pub fn get_points(&self) -> Vec<TelemetryPoint> {
        let data = self.data.read().unwrap();
        data.clone()
    }

    /// Get points for a specific time range
    pub fn get_points_in_range(&self, start: Duration, end: Duration) -> Vec<TelemetryPoint> {
        let data = self.data.read().unwrap();
        data.iter()
            .filter(|p| {
                let point_duration = p.captured_at.elapsed();
                point_duration <= end && point_duration >= start
            })
            .cloned()
            .collect()
    }

    /// Store the latest session info from the active plugin.
    pub fn update_session(&self, session: SessionInfo) {
        *self.session.write().unwrap() = Some(session);
    }

    /// Returns a clone of the most recently received session info.
    pub fn latest_session(&self) -> Option<SessionInfo> {
        self.session.read().unwrap().clone()
    }

    /// Get the latest point
    pub fn latest(&self) -> Option<TelemetryPoint> {
        let data = self.data.read().unwrap();
        data.last().cloned()
    }

    /// Clear all data
    pub fn clear(&self) {
        self.data.write().unwrap().clear();
        *self.session.write().unwrap() = None;
    }

    /// Returns the configured time window.
    pub fn window_duration(&self) -> Duration {
        self.window_duration
    }

    /// Get the number of points in the buffer
    pub fn len(&self) -> usize {
        self.data.read().unwrap().len()
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.data.read().unwrap().is_empty()
    }

    /// Remove old points outside the time window
    fn prune_old_points(&self, data: &mut Vec<TelemetryPoint>) {
        let now = std::time::Instant::now();
        let cutoff = now - self.window_duration;

        // Find the first point within the window
        let keep_from = data
            .iter()
            .position(|p| p.captured_at >= cutoff)
            .unwrap_or(data.len());

        // Keep at least min_points even if they're outside the window
        let keep_from = keep_from.min(data.len().saturating_sub(self.min_points));

        if keep_from > 0 {
            data.drain(..keep_from);
        }
    }
}

impl Default for TelemetryBuffer {
    fn default() -> Self {
        Self::new(Duration::from_secs(10))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_push_and_get() {
        let buffer = TelemetryBuffer::new(Duration::from_secs(10));
        buffer.push(
            VehicleTelemetry {
                throttle: 0.5,
                ..Default::default()
            },
            false,
        );
        assert_eq!(buffer.len(), 1);
        assert_eq!(buffer.get_points()[0].telemetry.throttle, 0.5);
    }

    #[test]
    fn test_latest() {
        let buffer = TelemetryBuffer::new(Duration::from_secs(10));
        assert!(buffer.latest().is_none());
        buffer.push(VehicleTelemetry::default(), false);
        assert!(buffer.latest().is_some());
    }

    #[test]
    fn test_clear_empties_buffer() {
        let buffer = TelemetryBuffer::new(Duration::from_secs(10));
        for _ in 0..5 {
            buffer.push(VehicleTelemetry::default(), false);
        }
        assert_eq!(buffer.len(), 5);
        buffer.clear();
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
        assert!(buffer.latest().is_none());
    }

    #[test]
    fn test_is_empty_on_new_buffer() {
        let buffer = TelemetryBuffer::new(Duration::from_secs(10));
        assert!(buffer.is_empty());
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_min_points_preserved_after_window_expires() {
        // Use a very short window so points expire quickly.
        let buffer = TelemetryBuffer::new(Duration::from_millis(50));
        let min_points = 10;

        for _ in 0..(min_points + 5) {
            buffer.push(VehicleTelemetry::default(), false);
        }
        assert_eq!(buffer.len(), min_points + 5);

        // Wait for all existing points to fall outside the window.
        thread::sleep(Duration::from_millis(100));

        // A new push triggers pruning; the min_points floor should keep old entries.
        buffer.push(VehicleTelemetry::default(), false);
        assert!(
            buffer.len() >= min_points,
            "expected at least {} points, got {}",
            min_points,
            buffer.len()
        );
    }

    #[test]
    fn test_window_pruning_removes_old_points() {
        let buffer = TelemetryBuffer::new(Duration::from_millis(50));
        let min_points = 10;

        // Overfill the buffer, then wait for points to age out.
        for _ in 0..(min_points * 3) {
            buffer.push(VehicleTelemetry::default(), false);
        }
        thread::sleep(Duration::from_millis(100));

        // After a push, expired entries should be pruned down to min_points.
        buffer.push(VehicleTelemetry::default(), false);
        assert!(buffer.len() <= min_points + 1); // +1 for the point just pushed
    }
}
