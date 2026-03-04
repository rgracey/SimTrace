//! Automobilista 2 plugin (pCars2 shared memory API)

#[cfg(windows)]
mod shared_memory;
mod ams2_plugin;

pub use ams2_plugin::Ams2Plugin;
