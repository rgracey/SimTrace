//! Plugin registry for managing available plugins
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
        return vec!["assetto_competizione".to_string()];

        #[cfg(not(windows))]
        return vec!["test".to_string()];
    }

    /// Get list of available plugin names
    pub fn available_plugins(&self) -> &[String] {
        &self.available_plugins
    }

    /// Activate a plugin by name
    pub fn activate(&mut self, name: &str) -> Result<()> {
        // Disconnect existing plugin
        if let Some(ref mut plugin) = self.active_plugin {
            plugin.disconnect();
        }

        // Create new plugin
        let plugin = crate::plugins::create_plugin(name)
            .ok_or_else(|| anyhow::anyhow!("Plugin '{}' not found", name))?;

        // Connect to game
        let mut plugin = plugin;
        plugin.connect()?;

        info!("Activated plugin: {}", plugin.name());
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
        let _registry = PluginRegistry::new();
        // Should have at least ACC on Windows
        #[cfg(windows)]
        assert!(!_registry.available_plugins.is_empty());
    }
}
