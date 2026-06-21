# DriveShare

Share any folder on your machine as a **network drive** over **WebDAV**, with a built-in **web dashboard** for browsing and managing files.

## Features

- **Web UI** — Dark-themed dashboard to browse, upload, download, delete, rename, and create files/folders
- **WebDAV Protocol** — Mount natively in Windows (Map Network Drive), macOS (Finder), and Linux (davfs2/GVFS)
- **Auto IP Detection** — Automatically detects your machine's real LAN IP for share links and logs
- **Multiple Shares** — Serve different directories under separate mount points
- **Background Daemon** — start/stop/status/restart commands with PID file management
- **Path Traversal Protection** — Canonical path validation blocks `../` attacks
- **Graceful Shutdown** — Handles Ctrl+C and SIGTERM cleanly
- **Dark Theme** — Easy on the eyes, works in any browser
- **Lightweight** — Single binary, no runtime dependencies
- **Cross-Platform** — Linux, Windows, macOS

## Quick Start

### Download

Grab the latest binary for your OS from the [Releases](https://github.com/anomalyco/driveshare/releases) page.

```bash
# Linux / macOS
chmod +x driveshare && ./driveshare

# Windows
driveshare.exe
```

### Build from Source

```bash
git clone https://github.com/anomalyco/driveshare.git
cd driveshare
cargo build --release
./target/release/driveshare
```

Open **http://localhost:8080** — the dashboard shows your server IP and share links.

## Connect from Other Devices

| Device | How to Connect |
|--------|---------------|
| **Browser** | `http://<ip>:8080` — full web UI |
| **Windows** | Map network drive → `http://<ip>:8080/shared` |
| **macOS** | Finder → Go → Connect to Server → `http://<ip>:8080/shared` |
| **Linux** | `sudo mount -t davfs http://<ip>:8080/shared /mnt/shared` |

The web UI displays the server's actual LAN IP — no need to check network settings.

### WSL2 (Windows Subsystem for Linux)

If running inside WSL2, other devices cannot access the server directly because WSL2 uses NAT networking.
You need to set up port forwarding from Windows to WSL2:

```powershell
# Run as Administrator in Windows PowerShell
scripts\wsl-port-forward.ps1
```

Or from inside WSL:
```bash
bash scripts/setup-wsl-network.sh
```

This forwards port 8443 (or your configured port) from Windows to WSL2 and opens the Windows Firewall.
After setup, other devices access: `http://<windows-ip>:8443`

To remove forwarding later:
```powershell
scripts\wsl-port-forward.ps1 -Remove
```

## Configuration

### Config File

Create `config.toml` or `driveshare.toml` in the working directory:

```toml
[server]
host = "0.0.0.0"
port = 8080

[[shares]]
name = "documents"
path = "/path/to/documents"
description = "Company documents"

[[shares]]
name = "shared"
path = "./shared"
description = "Shared folder"
```

Config search order:
1. Path from `--config` / `-c`
2. `./config.toml` or `./driveshare.toml`
3. `~/.config/driveshare/config.toml`
4. Built-in defaults (`0.0.0.0:8080`, share `./shared`)

### CLI Options

| Flag | Short | Description |
|------|-------|-------------|
| `--config <FILE>` | `-c` | Config file path |
| `--host <HOST>` | `-H` | Bind address override |
| `--port <PORT>` | `-P` | Port override |
| `--foreground` | | Run in foreground (daemon commands) |
| `--clean` | | Remove stale PID file (status command) |
| `--help` | `-h` | Print help |
| `--version` | `-V` | Print version |

## Daemon Mode

```bash
driveshare start     # Start background server
driveshare status    # Check if running
driveshare stop      # Stop server
driveshare restart   # Restart server
driveshare           # Run in foreground (default)
```

## systemd Service (Linux)

```bash
# Edit driveshare.service — set User and WorkingDirectory
sudo cp driveshare.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable driveshare
sudo systemctl start driveshare
```

## API Reference

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/` | GET | Web dashboard |
| `/api/ip` | GET | Server IP info `{"ip":"192.168.1.5","port":8080}` |
| `/api/shares` | GET | List shares |
| `/api/files/*` | GET | List directory (JSON) |
| `/*` | GET | Download file / WebDAV browse |
| `/*` | PUT | Upload file |
| `/*` | DELETE | Delete file/directory |
| `/*` | PROPFIND | WebDAV property discovery |
| `/*` | MKCOL | Create directory |
| `/*` | COPY | Copy (needs `Destination` header) |
| `/*` | MOVE | Move/rename (needs `Destination` header) |

## Cross-Platform Builds

Push a tag to automatically build for all platforms via GitHub Actions:

```bash
git tag v1.0
git push origin v1.0
```

Downloads include: Linux (x64, ARM64), Windows (x86, x64, ARM64), macOS (x64, ARM64).

## Project Structure

```
src/
├── main.rs           # Entry point, subcommand dispatch
├── cli.rs            # CLI argument parsing (clap)
├── config.rs         # TOML configuration loader
├── daemon.rs         # Daemon management (PID, start/stop/status)
├── server.rs         # HTTP server, IP detection, graceful shutdown
├── webdav.rs         # WebDAV protocol implementation
├── ui.rs             # Web UI + JSON API handlers
├── error.rs          # Error types and HTTP responses
├── dashboard.html    # Main web UI (HTML + CSS + JS)
└── browser.html      # Directory listing UI (WebDAV view)
```

## Tech Stack

- **Rust** — Performance, safety, single-binary deployment
- **Axum** — Async HTTP framework
- **Tokio** — Async runtime
- **WebDAV** — RFC 4918 compliant
- **No authentication** — Designed for trusted LAN use

## Development

```bash
# Watch mode
cargo watch -x run

# Test
cargo test

# Lint
cargo clippy

# Format
cargo fmt
```

## License

MIT
# driveshare
# driveshare
