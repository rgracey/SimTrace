//! Core telemetry collection and buffering.

pub mod buffer;
pub mod collector;
pub mod lap_store;
pub mod model;

pub use buffer::TelemetryBuffer;
pub use collector::DataCollector;
pub use lap_store::LapStore;
pub use model::{TelemetryData, TelemetryPoint, VehicleTelemetry};
