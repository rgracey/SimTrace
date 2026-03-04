//! Game plugin trait and registry

pub mod ams2;
pub mod assetto_competizione;
pub mod mock;
pub mod registry;
pub mod trait_;

pub use registry::PluginRegistry;
pub use trait_::{create_plugin, plugin_entries, GameConfig, GamePlugin};
