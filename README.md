# swarmctl

`swarmctl` is a lightweight, production-oriented CLI for managing Docker Swarm clusters. It is inspired by tools like `kubectl`, but designed specifically for Docker Swarm using Rust and the Bollard Docker API client.

This project is intended for DevOps workflows, homelab environments, and automated CI/CD pipelines where direct control over Docker Swarm is required.

---

## Features

* List swarm nodes, services, tasks, networks, secrets, configs, and stacks
* Clean, table-based output with ANSI color support
* JSON and YAML output formats
* **Interactive TUI Dashboard** - Real-time monitoring with keyboard navigation
* **Port Scanner** - Visualize and manage container port mappings* Stack deployment and management from Docker Compose files* **Cluster Information** - View swarm configuration and raft settings
* Native async Rust implementation using Tokio
* Direct integration with Docker API via Bollard

---

## Quick Start

### Build

```bash
# Install Rust first
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build
cargo build --release

# Or use Docker
make build-docker
```

### Run

```bash
# TUI Dashboard
./target/release/swarmctl dashboard

# List resources
./target/release/swarmctl get services
./target/release/swarmctl get nodes

# Port scanner
./target/release/swarmctl ports --tui

# Cluster info
./target/release/swarmctl cluster-info
```

---

## Documentation

For detailed build instructions and usage, see [BUILD.md](./BUILD.md).

---

## Project Structure

```
swarmctl/
├── Cargo.toml
├── Makefile
├── BUILD.md           # Detailed build and usage guide
├── README.md
└── src/
    ├── main.rs
    │
    ├── api/              # Docker API layer
    │   ├── client.rs
    │   ├── node.rs
    │   ├── service.rs
    │   ├── task.rs
    │   ├── network.rs
    │   ├── secret.rs
    │   ├── config.rs
    │   ├── stack.rs
    │   ├── swarm.rs
    │   └── port.rs
    │
    ├── cli/              # CLI commands
    │   ├── root.rs
    │   ├── get.rs
    │   ├── describe.rs
    │   ├── create.rs
    │   ├── delete.rs
    │   ├── scale.rs
    │   ├── logs.rs
    │   ├── ports.rs
    │   └── cluster.rs
    │
    ├── models/           # Output models
    │   ├── node.rs
    │   ├── service.rs
    │   ├── task.rs
    │   ├── network.rs
    │   ├── secret.rs
    │   ├── config.rs
    │   ├── stack.rs
    │   └── port.rs
    │
    ├── tui/              # TUI Dashboard
    │   └── mod.rs
    │
    └── utils/            # Shared utilities
        └── printer.rs
```

---

## Installation

### Prerequisites

* Rust (latest stable)
* Docker installed and running
* Access to a Docker Swarm manager node

### Build

```bash
cargo build --release
```

### Run

```bash
# Using Make
make build-release
make run

# Direct
./target/release/swarmctl --help
```

---

## Configuration

`swarmctl` connects to Docker using the `DOCKER_HOST` environment variable.

### Local Docker socket

```bash
export DOCKER_HOST=unix:///var/run/docker.sock
```

### Remote Docker daemon (TCP)

```bash
export DOCKER_HOST=tcp://<manager-ip>:2375
```

Example:

```bash
export DOCKER_HOST=tcp://192.168.5.65:2375
```

---

## Important Notes on Docker Configuration

To allow remote access, the Docker daemon must be configured to listen on a TCP socket.

Edit or create:

```
/etc/systemd/system/docker.service.d/override.conf
```

Add:

```ini
[Service]
ExecStart=
ExecStart=/usr/bin/dockerd -H unix:///var/run/docker.sock -H tcp://0.0.0.0:2375
```

Then reload and restart Docker:

```bash
sudo systemctl daemon-reexec
sudo systemctl restart docker
```

Verify:

```bash
ss -lntp | grep dockerd
```

---

## Security Warning

Exposing Docker over TCP without TLS allows full control over the host.

Recommended approaches:

* Use a private network
* Use Tailscale or VPN
* Restrict binding to a specific IP
* For production, enable TLS on port 2376

---

## Usage Examples

### TUI Dashboard

```bash
swarmctl dashboard
```

Navigate with Tab, 1-6 keys, j/k for scrolling, r to refresh.

### Port Scanner

```bash
# List all port mappings
swarmctl ports

# TUI visualization
swarmctl ports --tui

# Show available ports
swarmctl ports --available

# Filter by protocol
swarmctl ports --protocol tcp
swarmctl ports --protocol udp

# Custom port range
swarmctl ports --range-start 30000 --range-end 40000
```

### List Resources

```bash
swarmctl get services
swarmctl get nodes
swarmctl get tasks
swarmctl get networks
swarmctl get secrets
swarmctl get configs
swarmctl get stacks
```

### Stack Management

```bash
# Deploy a stack from a Compose file
swarmctl stack deploy -c docker-compose.yml my-stack

# List deployed stacks
swarmctl stack ls

# Remove a stack and its resources
swarmctl stack rm my-stack
```

### Output Formats

```bash
# Table (default)
swarmctl get services

# JSON
swarmctl get services -o json

# YAML
swarmctl get services -o yaml
```

### Describe Resources

```bash
swarmctl describe services my-service
swarmctl describe nodes node1
swarmctl describe networks my-network
```

### Cluster Info

```bash
swarmctl cluster-info
```

---

## Architecture Overview

`swarmctl` follows a layered design:

### API Layer

Handles communication with Docker using Bollard. Responsible for:
- Fetching nodes, services, tasks, networks, secrets, configs
- Port scanning and analysis
- Swarm cluster information

### CLI Layer

Handles user interaction and command parsing via Clap.

### Models Layer

Defines clean, display-ready structs separate from Docker's raw API objects.

### TUI Layer

Interactive terminal dashboard using ratatui for real-time monitoring.

### Utilities

Shared helpers such as table rendering and JSON/YAML serialization.

---

## Development Workflow

1. Add functionality in the API layer
2. Map API results into models
3. Expose via CLI commands
4. Format output via utilities

### Makefile Commands

```bash
make build           # Debug build
make build-release   # Release build
make build-docker    # Build in Docker
make clean           # Clean artifacts
make test            # Run tests
make run             # Run debug version
make install         # Install to ~/.local/bin
make fmt             # Format code
make clippy          # Lint code
```

---

## Troubleshooting

### Cannot connect to Docker daemon

```
Cannot connect to the Docker daemon
```

Ensure:
- Docker is running
- `DOCKER_HOST` is set correctly
- TCP port is exposed if using remote

---

### Not a swarm manager

```
This node is not a swarm manager
```

Ensure:
- You are connected to a manager node
- Verify with: `docker node ls`

---

### No nodes returned

Check:

```bash
docker info | grep Swarm
```

Expected:

```
Swarm: active
Is Manager: true
```

---

## Roadmap

Planned features:
- [x] Node listing and inspection
- [x] Service listing and inspection
- [x] Service scaling
- [x] Service logs
- [x] Stack deployment and management
- [x] Port scanner with TUI
- [x] Cluster information
- [ ] Remote build and deploy pipelines
- [ ] CI/CD integration

---

## Contributing

Contributions are welcome. The project is designed to be modular and easy to extend.

---

## License

MIT License
