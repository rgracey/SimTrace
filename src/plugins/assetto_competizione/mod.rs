//! Assetto Corsa Competizione plugin (Windows only — uses shared memory)

#[cfg(windows)]
mod shared_memory;

#[cfg(windows)]
mod mapping;

#[cfg(windows)]
mod acc_plugin;

#[cfg(windows)]
pub use acc_plugin::AccPlugin;
