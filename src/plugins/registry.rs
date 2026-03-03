//! Plugin registry for managing available plugins

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
        let mut plugins = Vec::new();

        // ACC is available on Windows
        #[cfg(windows)]
        plugins.push("assetto_competizione".to_string());

        // Test plugin for non-Windows development/testing
        #[cfg(not(windows))]
        plugins.push("test".to_string());

        // Add more plugins here as they're implemented
        plugins
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
            .map_or(false, |p| p.is_connected())
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
        // Should have at least ACC on Windows
        #[cfg(windows)]
        assert!(!registry.available_plugins.is_empty());
    }
}
