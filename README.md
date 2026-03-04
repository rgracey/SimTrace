# SimTrace

A lightweight sim racing telemetry overlay. Displays pedal inputs, steering angle, gear, and speed in real time as a transparent overlay on top of your game.

## Features

- **Pedal trace graph** — scrolling throttle, brake, and clutch history
- **Pedal bars** — live clutch, brake, and throttle with ABS indication
- **Steering wheel** — visual angle indicator with gear and speed readout
- **Transparent overlay** — drag to position, drag the corner to resize
- **Configurable** — opacity, FPS, speed unit, colours, and trace window length

## Supported games

| Game | Platform |
|------|----------|
| Assetto Corsa Competizione | Windows |

## Installation

Download the latest `.exe` from the [Releases](../../releases) page. No installer required — just run it.

## Usage

1. Start SimTrace before or after launching your game.
2. Select your game from the **⚙** config panel (top-right of the overlay).
3. Drag the title bar to reposition; drag the bottom-right corner to resize.
4. Click **Save** to persist your settings between sessions.

The overlay fades out when your cursor leaves it and reappears on hover.

## Building from source

Requires [Rust](https://rustup.rs/) (stable).

```sh
cargo build --release
```

The binary will be at `target/release/simtrace.exe` (Windows) or `target/release/simtrace` (other platforms).

## Settings

Settings are saved to:

- **Windows:** `%APPDATA%\simtrace\settings.toml`
- **macOS:** `~/Library/Application Support/simtrace/settings.toml`
- **Linux:** `~/.config/simtrace/settings.toml`
