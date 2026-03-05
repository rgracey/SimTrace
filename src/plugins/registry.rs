//! Plugin registry for managing available game plugins.
#![allow(dead_code)]

use crate::plugins::GamePlugin;
use anyhow::Result;
use tracing::info;

/// Registry of available game plugins
pub struct PluginRegistry {
    /// Currently active plugin
    active_plugin: Option<Box<dyn GamePlugin>>,
    /// List of available plugin names
    available_plugins: Vec<String>,
}

impl PluginRegistry {
    /// Create a new plugin registry
    pub fn new() -> Self {
        let available_plugins = Self::discover_plugins();
        Self {
            active_plugin: None,
            available_plugins,
        }
    }

    /// Discover available plugins
    fn discover_plugins() -> Vec<String> {
        #[cfg(windows)]
        return vec![
            "assetto_competizione".to_string(),
            "ams2".to_string(),
            "iracing".to_string(),
        ];

        #[cfg(not(windows))]
        return vec!["test".to_string()];
    }

    /// Get list of available plugin names
    pub fn available_plugins(&self) -> &[String] {
        &self.available_plugins
    }

    /// Activate a plugin by name. Registers the plugin even if the initial
    /// connection fails — the collector will retry automatically.
    pub fn activate(&mut self, name: &str) -> Result<()> {
        if let Some(ref mut plugin) = self.active_plugin {
            plugin.disconnect();
        }
        self.active_plugin = None;

        let mut plugin = crate::plugins::create_plugin(name)
            .ok_or_else(|| anyhow::anyhow!("Plugin '{}' not found", name))?;

        if let Err(e) = plugin.connect() {
            tracing::warn!(
                "Initial connection to '{}' failed: {e} (will retry automatically)",
                plugin.name()
            );
        }

        info!(
            "Plugin '{}' selected (connected: {})",
            plugin.name(),
            plugin.is_connected()
        );
        self.active_plugin = Some(plugin);
        Ok(())
    }

    /// Get the active plugin
    pub fn active_plugin(&self) -> Option<&dyn GamePlugin> {
        self.active_plugin.as_deref()
    }

    /// Get mutable access to the active plugin
    pub fn active_plugin_mut(&mut self) -> Option<&mut Box<dyn GamePlugin>> {
        self.active_plugin.as_mut()
    }

    /// Check if a plugin is active and connected
    pub fn is_connected(&self) -> bool {
        self.active_plugin
            .as_ref()
            .is_some_and(|p| p.is_connected())
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_plugins() {
        let registry = PluginRegistry::new();
        #[cfg(windows)]
        assert!(!registry.available_plugins().is_empty());
        #[cfg(not(windows))]
        let _ = registry; // always available on any platform
    }

    #[test]
    fn test_activate_mock_plugin_connects() {
        let mut registry = PluginRegistry::new();
        assert!(!registry.is_connected());
        registry.activate("mock").unwrap();
        assert!(registry.is_connected());
    }

    #[test]
    fn test_activate_unknown_plugin_errors() {
        let mut registry = PluginRegistry::new();
        assert!(registry.activate("does_not_exist").is_err());
    }

    #[test]
    fn test_activate_replaces_previous_plugin() {
        let mut registry = PluginRegistry::new();
        registry.activate("mock").unwrap();
        assert!(registry.is_connected());
        // Activating again replaces the existing plugin.
        registry.activate("mock").unwrap();
        assert!(registry.is_connected());
    }
}
