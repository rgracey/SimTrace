//! iRacing plugin (iRacing SDK shared memory API)

mod iracing_plugin;
#[cfg(windows)]
mod shared_memory;

pub use iracing_plugin::IracingPlugin;
