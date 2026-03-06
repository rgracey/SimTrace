//! egui renderer for telemetry visualization

pub mod app;
pub mod lap_comparison;
pub mod phase_plot;
pub mod steering_wheel;
pub mod trace_graph;

pub use app::SimTraceApp;
pub use lap_comparison::LapComparison;
pub use phase_plot::PhasePlot;
pub use steering_wheel::SteeringWheel;
pub use trace_graph::TraceGraph;
