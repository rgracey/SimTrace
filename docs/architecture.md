# SimTrace Architecture

## Overview

SimTrace is a real-time telemetry visualization tool for sim racing. It displays brake/throttle traces on a scrolling graph and a rotating steering wheel visualization, with support for multiple racing games through a pluggable architecture.

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         SimTrace                                │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────────┐    ┌─────────────────┐    ┌─────────────┐ │
│  │   Game Plugin   │───▶│   Common Model  │───▶│  Renderer   │ │
│  │   (ACC, AMS2,   │    │   (Normalized)  │    │   (egui)    │ │
│  │    iRacing...)  │    │                 │    │             │ │
│  └─────────────────┘    └────────┬────────┘    └─────────────┘ │
│         ▲                        │                              │
│         │                        ▼                              │
│  ┌─────────────────┐    ┌─────────────────┐                     │
│  │  Data Collector │    │  Buffer/Store   │                     │
│  │  (Shared Memory,│    │  (10s sliding   │                     │
│  │   UDP, etc.)    │    │   window)       │                     │
│  └─────────────────┘    └─────────────────┘                     │
└─────────────────────────────────────────────────────────────────┘
```

## Core Principles

1. **Decoupled Architecture**: Data collection and visualization are completely separate
2. **Pluggable Game Support**: New games added via plugins implementing a common interface
3. **Common Telemetry Model**: All games map to a normalized data representation
4. **Configurability**: Visual appearance and behavior are user-configurable
5. **Real-Time Performance**: Target 60Hz visualization refresh rate

## Components

### 1. Game Plugin Layer

Each supported game has a plugin that handles:
- Game-specific data extraction (shared memory, UDP, files, etc.)
- Mapping game telemetry to the common model
- Game-specific calibration (steering angle ranges, pedal travel, etc.)

**Plugin Interface:**
```rust
trait GamePlugin: Send + Sync {
    fn name(&self) -> &str;
    fn connect(&mut self) -> Result<()>;
    fn disconnect(&mut self);
    fn is_connected(&self) -> bool;
    fn read_telemetry(&mut self) -> Result<Option<TelemetryData>>;
    fn get_config(&self) -> GameConfig;  // steering limits, etc.
}
```

### 2. Data Collector

The data collector:
- Polls the active game plugin at a fixed interval (target: 60Hz+)
- Validates and normalizes incoming data
- Pushes data to the shared buffer
- Handles reconnection logic

### 3. Common Telemetry Model

Normalized data structure that all games map to:

```rust
struct TelemetryData {
    timestamp: Duration,
    vehicle: VehicleTelemetry,
    session: SessionInfo,  // optional, for future features
}

struct VehicleTelemetry {
    // Pedals (0.0 to 1.0)
    throttle: f32,
    brake: f32,
    clutch: f32,  // optional
    
    // Steering (-1.0 to 1.0, or angle in degrees)
    steering_angle: f32,  // degrees, negative = left
    
    // Vehicle state
    speed: f32,      // m/s
    gear: i32,
    rpm: f32,
    
    // ABS/Traction Control
    abs_active: bool,
    tc_active: bool,
    
    // Position (optional, for future features)
    track_position: f32,  // 0.0 to 1.0 along track
}
```

### 4. Data Buffer

Ring buffer storing the telemetry history:
- Configurable time window (default: 10 seconds)
- Stores raw telemetry points with timestamps
- Provides interpolated data for smooth rendering
- Thread-safe for producer/consumer pattern

```rust
struct TelemetryBuffer {
    window_duration: Duration,
    data: Vec<TelemetryPoint>,
    lock: Mutex<...>,
}

struct TelemetryPoint {
    timestamp: Instant,
    telemetry: VehicleTelemetry,
    // ABS state is stored with the point for persistent coloring
    abs_active: bool,
}
```

### 5. Renderer (egui)

The visualization layer:
- **Trace Graph**: Scrolling graph showing brake/throttle over time
  - Green line: throttle
  - Red line: brake (when ABS inactive)
  - Orange segments: brake + ABS active (color persists as graph scrolls)
  - Y-axis: 0-100% pedal position
  - X-axis: configurable time window
- **Steering Display**: Rotating wheel graphic
  - Generic wheel design
  - Rotation based on steering angle
  - Center position = straight ahead

**ABS Color Behavior**:
- Brake pedal value is plotted normally (Y-axis position unchanged)
- Each brake data point stores its ABS state at capture time
- When rendering, brake segments use `abs_active` color if ABS was active at that moment
- Colored segments persist on the graph as it scrolls (not re-evaluated per frame)

**Renderer Features:**
- Configurable color scheme
- Configurable graph scale and window
- FPS counter and debug info
- Minimal UI for plugin selection

## Data Flow

```
┌──────────────┐
│ Game Plugin  │ 1. Read raw telemetry
└──────┬───────┘
       │
       ▼
┌──────────────┐
│   Common     │ 2. Map to normalized model
│   Model      │
└──────┬───────┘
       │
       ▼
┌──────────────┐
│    Buffer    │ 3. Store in ring buffer
└──────┬───────┘
       │
       ▼
┌──────────────┐
│   Renderer   │ 4. Read & visualize
└──────────────┘
```

### Threading Model

- **Main Thread**: egui rendering (60Hz target)
- **Collector Thread**: Data collection from game (60Hz+)
- **Shared Buffer**: Lock-free or mutex-protected ring buffer

## Assetto Corsa Competizione Plugin

ACC provides telemetry via shared memory:

**Data Source**: Windows shared memory (`acSM`)

**Key Data Points**:
- `steeringAngle`: degrees (-900 to +900 typical)
- `throttle`: 0-1
- `brake`: 0-1
- `abs`: 0-1 (float representing ABS intervention level)
- `rpm`: engine RPM
- `gear`: current gear
- `speed`: km/h

**Mapping to Common Model**:
```rust
impl From<ACCTelemetry> for VehicleTelemetry {
    fn from(acc: ACCTelemetry) -> Self {
        Self {
            throttle: acc.throttle,
            brake: acc.brake,
            steering_angle: acc.steeringAngle,
            abs_active: acc.abs > 0.1,  // threshold
            // ...
        }
    }
}
```

## Configuration

### Color Scheme (config.toml)
```toml
[colors]
throttle = "#00FF00"
brake = "#FF0000"
abs_active = "#FFA500"
background = "#1A1A1A"
grid = "#333333"
```

### Graph Settings
```toml
[graph]
window_seconds = 10
update_rate_hz = 60
show_grid = true
```

## File Structure

```
SimTrace/
├── Cargo.toml
├── README.md
├── docs/
│   ├── architecture.md      # This file
│   ├── plugin-guide.md      # How to write game plugins
│   └── configuration.md     # Configuration reference
├── src/
│   ├── main.rs              # Application entry point
│   ├── lib.rs               # Library root
│   ├── core/
│   │   ├── mod.rs
│   │   ├── model.rs         # Common telemetry model
│   │   ├── buffer.rs        # Telemetry buffer
│   │   └── collector.rs     # Data collection loop
│   ├── plugins/
│   │   ├── mod.rs
│   │   ├── trait.rs         # GamePlugin trait
│   │   └── assetto_competizione/
│   │       ├── mod.rs
│   │       ├── shared_memory.rs
│   │       └── mapping.rs
│   ├── renderer/
│   │   ├── mod.rs
│   │   ├── app.rs           # Main egui app
│   │   ├── trace_graph.rs   # Scrolling graph
│   │   └── steering_wheel.rs
│   └── config/
│       ├── mod.rs
│       └── settings.rs
└── tests/
    └── integration/
```

## Technology Stack

| Layer | Technology | Rationale |
|-------|------------|-----------|
| Language | Rust | Performance, safety, cross-platform |
| GUI | egui | Immediate mode, easy rendering, Rust-native |
| Build | Cargo | Standard Rust tooling |
| Config | TOML | Human-readable, widely used |
| Logging | tracing + tracing-subscriber | Flexible, structured logging |

## Future Considerations

1. **Additional Games**: AMS2 (shared memory), iRacing (IRSDK)
2. **Lap Comparison**: Overlay multiple sessions
3. **Telemetry Export**: CSV/JSON export for analysis
4. **Custom Widgets**: User-defined visualizations
5. **Network Telemetry**: UDP streaming for headless setups

## Non-Goals

- Recording/playback (live only for now)
- Game integration (standalone overlay window)
- Mobile platforms (Windows primary, Linux/Mac optional)