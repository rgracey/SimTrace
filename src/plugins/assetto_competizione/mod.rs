//! Assetto Corsa Competizione plugin

#[cfg(windows)]
mod shared_memory;

#[cfg(windows)]
mod mapping;

#[cfg(windows)]
mod acc_plugin;

#[cfg(windows)]
pub use acc_plugin::AccPlugin;

/// Mock plugin for non-Windows platforms (development/testing)
#[cfg(not(windows))]
mod mock_plugin;

#[cfg(not(windows))]
pub use mock_plugin::AccPlugin;