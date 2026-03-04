//! Polls the active game plugin and feeds telemetry into the shared buffer.

use std::sync::Arc;
use std::time::Duration;

use tracing::{error, info};

use crate::core::TelemetryBuffer;
use crate::plugins::PluginRegistry;

/// Owns the plugin registry and the telemetry buffer.
/// Call [`poll`] from the UI thread each frame to ingest the latest game data.
pub struct DataCollector {
    buffer: Arc<TelemetryBuffer>,
    plugin_registry: PluginRegistry,
}

impl DataCollector {
    /// Creates a new collector with a buffer large enough to hold
    /// `buffer_window_secs` seconds of history.
    pub fn new(buffer_window_secs: u64) -> Self {
        Self {
            buffer: Arc::new(TelemetryBuffer::new(Duration::from_secs(
                buffer_window_secs,
            ))),
            plugin_registry: PluginRegistry::new(),
        }
    }

    /// Returns a shared reference to the telemetry buffer.
    pub fn buffer(&self) -> Arc<TelemetryBuffer> {
        self.buffer.clone()
    }

    /// Activates and connects to the named plugin.
    /// Any previously active plugin is disconnected first.
    pub fn activate_plugin(&mut self, name: &str) -> anyhow::Result<()> {
        self.plugin_registry.activate(name)?;
        info!("Activated plugin: {}", name);
        Ok(())
    }

    /// Reads the latest telemetry from the active plugin and pushes it into
    /// the buffer. Should be called once per frame on the UI thread.
    ///
    /// # Note
    /// `read_telemetry` is assumed to be non-blocking. If a future plugin
    /// involves I/O, move polling to a dedicated background thread instead.
    pub fn poll(&mut self) {
        if let Some(plugin) = self.plugin_registry.active_plugin_mut() {
            match plugin.read_telemetry() {
                Ok(Some(data)) => {
                    let abs_active = data.vehicle.abs_active;
                    self.buffer.push(data.vehicle, abs_active);
                }
                Ok(None) => {} // no data yet
                Err(e) => {
                    error!("Error reading telemetry: {}", e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_starts_with_empty_buffer() {
        let collector = DataCollector::new(10);
        assert!(collector.buffer().is_empty());
    }

    #[test]
    fn test_activate_mock_plugin() {
        let mut collector = DataCollector::new(10);
        // Activation succeeds without error.
        collector.activate_plugin("mock").unwrap();
    }

    #[test]
    fn test_poll_populates_buffer() {
        let mut collector = DataCollector::new(10);
        collector.activate_plugin("mock").unwrap();
        assert!(collector.buffer().is_empty());
        collector.poll();
        assert_eq!(collector.buffer().len(), 1);
    }
}
