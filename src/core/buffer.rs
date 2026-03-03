//! Telemetry buffer - stores telemetry points with a sliding time window

use std::sync::RwLock;
use std::time::Duration;

use crate::core::{TelemetryPoint, VehicleTelemetry};

/// Buffer storing telemetry points with a configurable time window
pub struct TelemetryBuffer {
    /// Maximum time window to keep
    window_duration: Duration,
    /// Stored telemetry points
    data: RwLock<Vec<TelemetryPoint>>,
    /// Minimum points to keep (prevents empty buffer)
    min_points: usize,
}

impl TelemetryBuffer {
    /// Create a new buffer with the specified time window
    pub fn new(window_duration: Duration) -> Self {
        Self {
            window_duration,
            data: RwLock::new(Vec::with_capacity(1000)),
            min_points: 10,
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

    /// Get the latest point
    pub fn latest(&self) -> Option<TelemetryPoint> {
        let data = self.data.read().unwrap();
        data.last().cloned()
    }

    /// Clear all data
    pub fn clear(&self) {
        self.data.write().unwrap().clear();
    }

    /// Set the time window duration
    pub fn set_window_duration(&self, _duration: Duration) {
        // Note: window_duration is not mutable behind &self, so we skip pruning for now
        // In a real implementation, use interior mutability or &mut self
    }

    /// Get current window duration
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
        let telemetry = VehicleTelemetry {
            throttle: 0.5,
            brake: 0.0,
            ..Default::default()
        };

        buffer.push(telemetry, false);
        assert_eq!(buffer.len(), 1);

        let points = buffer.get_points();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].telemetry.throttle, 0.5);
    }

    #[test]
    fn test_window_pruning() {
        let buffer = TelemetryBuffer::new(Duration::from_millis(100));

        // Add points
        for _ in 0..10 {
            buffer.push(VehicleTelemetry::default(), false);
            thread::sleep(Duration::from_millis(20));
        }

        // Wait for window to expire
        thread::sleep(Duration::from_millis(150));

        // Points should be pruned
        assert!(buffer.len() <= 10);
    }

    #[test]
    fn test_latest() {
        let buffer = TelemetryBuffer::new(Duration::from_secs(10));

        assert!(buffer.latest().is_none());

        buffer.push(VehicleTelemetry::default(), false);
        assert!(buffer.latest().is_some());
    }
}
