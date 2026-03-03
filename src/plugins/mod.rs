//! Game plugin trait and registry

pub mod registry;
pub mod trait_;

pub use registry::PluginRegistry;
pub use trait_::{create_plugin, GamePlugin};
