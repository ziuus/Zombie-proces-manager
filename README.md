# Orphan Process Manager (OPM)

A high-performance, lightweight terminal utility built in Rust to detect, monitor, manage orphaned background processes, and **launch & track your own apps**.

## Features

- **🧟 Orphan Detection** — Finds processes reparented to PID 1 that match common dev tool patterns (Node.js, Python, Chrome, etc.)
- **🚀 App Launcher** — Press `a`, type any shell command, and OPM spawns it and tracks its entire process tree
- **📡 Port Mapping** — Identifies which processes are holding local TCP ports — no more "Address already in use" mysteries
- **📊 Resource Monitoring** — Real-time memory usage tracking, sorted by consumption
- **👁️ View Modes** — Toggle between orphan processes only, tracked processes only, or all processes with `Tab`
- **💀 Interactive Reaper** — Kill processes individually with `x`/`Enter`; tracked root processes kill their children too
- **🟢 Visual Distinction** — Tracked processes shown in green, orphans in yellow, root tracked processes in bold green
- **🦀 Zero-Bloat** — Built with Rust and `ratatui` — single binary, minimal footprint

## Controls

| Key | Action |
|-----|--------|
| `a` | Launch a new app/command |
| `Tab` | Cycle view mode (ORPHANS → ALL PROCS → TRACKED) |
| `k` / `↑` | Move selection up |
| `j` / `↓` | Move selection down |
| `x` / `Enter` | Terminate selected process (kills children for tracked roots) |
| `r` | Manual refresh |
| `q` / `Esc` | Exit application |

When in launch input mode (`a`):
- Type any shell command (supports pipes, redirects, etc.)
- `Enter` to launch, `Esc` to cancel

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable)
- Linux (uses `/proc` based process inspection)

### Build & Run

```bash
git clone https://github.com/ziuus/Zombie-proces-manager.git
cd Zombie-proces-manager
cargo run --release
```

## Tech Stack

- **Core:** Rust
- **TUI:** [Ratatui](https://github.com/ratatui/ratatui) + [Crossterm](https://github.com/crossterm-rs/crossterm)
- **Process Inspection:** `sysinfo` + `procfs`

## License

MIT
