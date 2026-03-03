//! Data collector - polls game plugin and feeds telemetry buffer
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tracing::{error, info};

use crate::core::TelemetryBuffer;
use crate::plugins::PluginRegistry;

/// Configuration for the data collector
#[derive(Debug, Clone)]
pub struct CollectorConfig {
    /// Update rate in Hz
    pub update_rate_hz: u32,
    /// Time window for the buffer
    pub buffer_window_secs: u64,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            update_rate_hz: 60,
            buffer_window_secs: 10,
        }
    }
}

/// Data collector that polls the game plugin and stores telemetry
pub struct DataCollector {
    pub config: CollectorConfig,
    buffer: Arc<TelemetryBuffer>,
    plugin_registry: PluginRegistry,
}

impl DataCollector {
    /// Create a new data collector
    pub fn new(config: CollectorConfig) -> Self {
        let buffer = Arc::new(TelemetryBuffer::new(Duration::from_secs(
            config.buffer_window_secs,
        )));
        let plugin_registry = PluginRegistry::new();

        Self {
            config,
            buffer,
            plugin_registry,
        }
    }

    /// Get the telemetry buffer
    pub fn buffer(&self) -> Arc<TelemetryBuffer> {
        self.buffer.clone()
    }

    /// Get available plugins
    pub fn available_plugins(&self) -> &[String] {
        self.plugin_registry.available_plugins()
    }

    /// Activate and connect to a plugin
    pub fn activate_plugin(&mut self, name: &str) -> Result<()> {
        self.plugin_registry.activate(name)?;
        info!("Activated plugin: {}", name);
        Ok(())
    }

    /// Poll the plugin and push telemetry to the buffer
    /// Call this from the main loop at the desired rate
    pub fn poll(&mut self) {
        if let Some(plugin) = self.plugin_registry.active_plugin_mut() {
            match plugin.read_telemetry() {
                Ok(Some(data)) => {
                    let abs_active = data.vehicle.abs_active;
                    self.buffer.push(data.vehicle, abs_active);
                }
                Ok(None) => {
                    // No data available yet
                }
                Err(e) => {
                    error!("Error reading telemetry: {}", e);
                }
            }
        }
    }

    /// Check if connected to a game
    pub fn is_connected(&self) -> bool {
        self.plugin_registry.is_connected()
    }

    /// Get the active plugin
    pub fn active_plugin(&self) -> Option<&dyn crate::plugins::GamePlugin> {
        self.plugin_registry.active_plugin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_creation() {
        let config = CollectorConfig::default();
        let collector = DataCollector::new(config);
        assert!(!collector.is_connected());
    }
}
