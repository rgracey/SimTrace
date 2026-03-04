//! Automobilista 2 plugin (pCars2 shared memory API)

mod ams2_plugin;
#[cfg(windows)]
mod shared_memory;

pub use ams2_plugin::Ams2Plugin;
