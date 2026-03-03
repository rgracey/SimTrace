# Plugin Development Guide

## Overview

SimTrace supports multiple racing games through a plugin architecture. Each plugin implements the `GamePlugin` trait and maps game-specific telemetry to the common `VehicleTelemetry` model.

## Creating a New Plugin

### 1. Create Plugin Module

Add a new directory under `src/plugins/`:

```
src/plugins/
└── your_game/
    ├── mod.rs
    ├── data_source.rs    # Shared memory, UDP, etc.
    └── mapping.rs        # Map to common model
```

### 2. Implement the Plugin Trait

```rust
use crate::core::model::{TelemetryData, VehicleTelemetry};
use crate::plugins::GamePlugin;
use anyhow::Result;

pub struct YourGamePlugin {
    // Plugin state (connection handles, buffers, etc.)
    connected: bool,
}

impl GamePlugin for YourGamePlugin {
    fn name(&self) -> &str {
        "Your Game"
    }

    fn connect(&mut self) -> Result<()> {
        // Initialize connection to game
        // - Open shared memory segment
        // - Connect to UDP port
        // - Start file watcher
        self.connected = true;
        Ok(())
    }

    fn disconnect(&mut self) {
        // Clean up resources
        self.connected = false;
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn read_telemetry(&mut self) -> Result<Option<TelemetryData>> {
        // 1. Read raw data from game
        let raw = self.data_source.read()?;
        
        // 2. Map to common model
        let telemetry = self.map_to_model(raw);
        
        // 3. Wrap with timestamp
        Ok(Some(TelemetryData {
            timestamp: std::time::Duration::from_millis(
                raw.timestamp_ms
            ),
            vehicle: telemetry,
            session: Default::default(),
        }))
    }

    fn get_config(&self) -> GameConfig {
        GameConfig {
            max_steering_angle: 540.0,  // degrees
            // ... other game-specific settings
        }
    }
}
```

### 3. Register the Plugin

Add to `src/plugins/mod.rs`:

```rust
mod your_game;

pub fn create_plugin(name: &str) -> Option<Box<dyn GamePlugin>> {
    match name.to_lowercase().as_str() {
        "assetto corsa competizione" | "acc" => {
            Some(Box::new(assetto_competizione::AccPlugin::new()))
        }
        "your game" | "yourgame" => {
            Some(Box::new(your_game::YourGamePlugin::new()))
        }
        _ => None,
    }
}
```

## Data Source Patterns

### Shared Memory (Windows)

```rust
use memmap2::MmapMut;

pub struct SharedMemorySource {
    mmap: MmapMut,
    offset: usize,
}

impl SharedMemorySource {
    pub fn new(name: &str) -> Result<Self> {
        let mmap = unsafe {
            MmapMut::map_anon(size_of::<RawTelemetry>())?
        };
        Ok(Self { mmap, offset: 0 })
    }

    pub fn read(&mut self) -> Result<RawTelemetry> {
        // Copy from shared memory
        let data = unsafe {
            &*(self.mmap.as_ptr() as *const RawTelemetry)
        };
        Ok(*data)
    }
}
```

### UDP Network

```rust
use std::net::UdpSocket;

pub struct UdpSource {
    socket: UdpSocket,
}

impl UdpSource {
    pub fn new(port: u16) -> Result<Self> {
        let socket = UdpSocket::bind(("0.0.0.0", port))?;
        socket.set_nonblocking(true)?;
        Ok(Self { socket })
    }

    pub fn read(&mut self) -> Result<Option<RawTelemetry>> {
        let mut buf = [0u8; 1024];
        match self.socket.recv_from(&mut buf) {
            Ok((len, _)) => {
                let telemetry = parse_udp_packet(&buf[..len])?;
                Ok(Some(telemetry))
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
```

## Mapping to Common Model

### Normalization

Different games report data in different formats. Normalize to common ranges:

| Field | Common Model | Normalization |
|-------|--------------|---------------|
| Throttle | 0.0 - 1.0 | Direct or divide by max |
| Brake | 0.0 - 1.0 | Direct or divide by max |
| Steering | Degrees | Use game's max angle |
| Speed | m/s | Convert from km/h or mph |

### Example Mapping

```rust
impl YourGamePlugin {
    fn map_to_model(&self, raw: RawYourGameTelemetry) -> VehicleTelemetry {
        VehicleTelemetry {
            throttle: (raw.throttle_pct / 100.0).clamp(0.0, 1.0),
            brake: (raw.brake_pct / 100.0).clamp(0.0, 1.0),
            clutch: raw.clutch_pct / 100.0,
            steering_angle: raw.steering_angle_deg,
            speed: raw.speed_kmh / 3.6,  // km/h to m/s
            gear: raw.gear as i32,
            rpm: raw.rpm as f32,
            abs_active: raw.abs_flag == 1,
            tc_active: raw.tc_level > 0,
            track_position: raw.distance_on_track / raw.track_length,
        }
    }
}
```

## Testing Your Plugin

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_steering_mapping() {
        let plugin = YourGamePlugin::new();
        let raw = RawYourGameTelemetry {
            steering_angle_deg: 270.0,
            // ...
        };
        let model = plugin.map_to_model(raw);
        assert_eq!(model.steering_angle, 270.0);
    }
}
```

### Integration Tests

Test with real game data by running the game and verifying visualization.

## Configuration

Plugins can provide game-specific defaults in `config.toml`:

```toml
[your_game]
max_steering_angle = 900
pedal_deadzone = 0.02
abs_threshold = 0.1
```

## Debugging

Use the `tracing` crate for diagnostic output:

```rust
use tracing::{debug, info, warn};

fn read_telemetry(&mut self) -> Result<Option<TelemetryData>> {
    debug!("Reading telemetry from {}", self.name());
    
    match self.data_source.read() {
        Ok(data) => {
            info!("Telemetry read successfully");
            Ok(Some(self.map_to_model(data)))
        }
        Err(e) => {
            warn!("Failed to read telemetry: {}", e);
            Err(e)
        }
    }
}
```

## Known Game APIs

| Game | Data Source | Notes |
|------|-------------|-------|
| Assetto Corsa Competizione | Shared Memory | `acSM` memory segment |
| Assetto Corsa | Shared Memory | `sim` structure |
| Automobilista 2 | Shared Memory | Project CARS SDK |
| iRacing | IRSDK | Official SDK provided |
| rFactor 2 | Plugin API | In-game plugin required |

## Best Practices

1. **Handle Disconnections**: Games may restart; plugins should reconnect gracefully
2. **Validate Data**: Check for impossible values (negative speed, >100% throttle)
3. **Document Mapping**: Comment non-obvious conversions
4. **Test with Real Data**: Simulation data may not match real game behavior
5. **Respect Game Resources**: Don't hold shared memory locks longer than needed