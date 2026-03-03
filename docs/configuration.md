# Configuration Reference

## Configuration File

SimTrace uses a TOML configuration file (`config.toml`) in the application directory.

## Full Example

```toml
# Application settings
[app]
title = "SimTrace"
width = 1280
height = 720
fps_limit = 60
fullscreen = false

# Data collection settings
[collector]
update_rate_hz = 60
plugin = "assetto_competizione"
reconnect_delay_ms = 1000

# Graph/trace visualization
[graph]
window_seconds = 10
show_grid = true
show_legend = true
line_width = 2.0
point_size = 3.0

# Color scheme
[colors]
# Pedals
throttle = "#00FF00"
brake = "#FF0000"
abs_active = "#FFA500"

# Graph elements
background = "#1A1A1A"
grid = "#333333"
text = "#FFFFFF"
legend = "#CCCCCC"

# Steering wheel
[steering_wheel]
size = 200
color = "#444444"
center_color = "#666666"
text_color = "#FFFFFF"
show_angle = false

# Game-specific settings
[assetto_competizione]
shared_memory_name = "acSM"
max_steering_angle = 900.0
pedal_deadzone = 0.01

[automobilista2]
shared_memory_name = "ProjectCars2Game"
max_steering_angle = 1080.0
pedal_deadzone = 0.02

[iRacing]
# iRacing uses IRSDK, no shared memory config needed
max_steering_angle = 1080.0
pedal_deadzone = 0.01
```

## Settings Reference

### `[app]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `title` | string | "SimTrace" | Window title |
| `width` | integer | 1280 | Window width in pixels |
| `height` | integer | 720 | Window height in pixels |
| `fps_limit` | integer | 60 | Maximum frames per second |
| `fullscreen` | boolean | false | Fullscreen mode |

### `[collector]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `update_rate_hz` | integer | 60 | Data collection frequency |
| `plugin` | string | "assetto_competizione" | Active game plugin |
| `reconnect_delay_ms` | integer | 1000 | Delay before reconnection attempt |

### `[graph]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `window_seconds` | float | 10 | Time window displayed |
| `show_grid` | boolean | true | Show grid lines |
| `show_legend` | boolean | true | Show legend |
| `line_width` | float | 2.0 | Trace line thickness |
| `point_size` | float | 3.0 | Data point marker size |

### `[colors]`

All colors in hex format (`#RRGGBB`).

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `throttle` | color | #00FF00 | Throttle trace color |
| `brake` | color | #FF0000 | Brake trace color |
| `abs_active` | color | #FFA500 | Brake+ABS segment color |
| `background` | color | #1A1A1A | Graph background |
| `grid` | color | #333333 | Grid line color |
| `text` | color | #FFFFFF | Text color |
| `legend` | color | #CCCCCC | Legend text color |

### `[steering_wheel]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `size` | integer | 200 | Wheel diameter in pixels |
| `color` | color | #444444 | Wheel rim color |
| `center_color` | color | #666666 | Center hub color |
| `text_color` | color | #FFFFFF | Text/numbers color |
| `show_angle` | boolean | false | Display steering angle value |

### `[game_specific]`

Game-specific settings vary by plugin. See plugin documentation.

## Runtime Configuration

Some settings can be changed at runtime via the UI:

- Window size
- Graph time window
- Color scheme (preset themes)
- Plugin selection (requires reconnect)

## Preset Themes

### Dark (Default)

```toml
[colors]
throttle = "#00FF00"
brake = "#FF0000"
abs_active = "#FFA500"
background = "#1A1A1A"
grid = "#333333"
text = "#FFFFFF"
```

### Light

```toml
[colors]
throttle = "#00AA00"
brake = "#CC0000"
abs_active = "#FF8800"
background = "#F5F5F5"
grid = "#DDDDDD"
text = "#333333"
```

### High Contrast

```toml
[colors]
throttle = "#00FF00"
brake = "#FF0000"
abs_active = "#FFFF00"
background = "#000000"
grid = "#555555"
text = "#FFFFFF"
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `SIMTRACE_CONFIG` | Override config file path |
| `SIMTRACE_LOG` | Enable logging to file |
| `RUST_LOG` | Logging level (error, warn, info, debug, trace) |

## Validation

Invalid configuration values are handled as follows:

- **Missing values**: Fall back to defaults
- **Out of range**: Clamp to valid range
- **Invalid format**: Log warning, use default

Example: If `window_seconds` is negative, it will be clamped to minimum 1 second.