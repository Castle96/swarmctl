# swarmctl

`swarmctl` is a lightweight, production-oriented CLI for managing Docker Swarm clusters. It is inspired by tools like `kubectl`, but designed specifically for Docker Swarm using Rust and the Bollard Docker API client.

This project is intended for DevOps workflows, homelab environments, and automated CI/CD pipelines where direct control over Docker Swarm is required.

---

## Features

* List swarm nodes
* List swarm services
* Clean, table-based output
* Modular architecture for future expansion
* Native async Rust implementation using Tokio
* Direct integration with Docker API via Bollard

---

## Project Structure

```
swarmctl/
├── Cargo.toml
└── src/
    ├── main.rs
    │
    ├── api/                  # Docker API layer
    │   ├── mod.rs
    │   ├── client.rs         # Docker client wrapper
    │   ├── node.rs           # Node API calls
    │   └── service.rs        # Service API calls
    │
    ├── cli/                  # CLI commands
    │   ├── mod.rs
    │   ├── root.rs           # Command routing
    │   ├── node.rs           # Node CLI logic
    │   └── service.rs        # Service CLI logic
    │
    ├── models/               # Output models
    │   ├── mod.rs
    │   ├── node.rs
    │   └── service.rs
    │
    └── utils/                # Shared utilities
        ├── mod.rs
        └── printer.rs        # Table formatting
```

---

## Installation

### Prerequisites

* Rust (latest stable)
* Docker installed and running
* Access to a Docker Swarm manager node

### Build

```
cargo build --release
```

### Run

```
cargo run -- node list
```

---

## Configuration

`swarmctl` connects to Docker using the `DOCKER_HOST` environment variable.

### Local Docker socket

```
export DOCKER_HOST=unix:///var/run/docker.sock
```

### Remote Docker daemon (TCP)

```
export DOCKER_HOST=tcp://<manager-ip>:2375
```

Example:

```
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

```
[Service]
ExecStart=
ExecStart=/usr/bin/dockerd -H unix:///var/run/docker.sock -H tcp://0.0.0.0:2375
```

Then reload and restart Docker:

```
sudo systemctl daemon-reexec
sudo systemctl restart docker
```

Verify:

```
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

## Usage

### List Nodes

```
swarmctl node list
```

Displays:

* Node ID
* Hostname
* Status
* Availability
* Manager reachability

---

### List Services

```
swarmctl service list
```

Displays:

* Service ID
* Name
* Mode (replicated/global)
* Replica count
* Container image

---

## Architecture Overview

`swarmctl` follows a layered design:

### API Layer

Handles communication with Docker using Bollard. This layer is responsible for:

* Fetching nodes
* Fetching services
* Future: scaling, updates, logs

### CLI Layer

Handles user interaction and command parsing via Clap.

### Models Layer

Defines clean, display-ready structs separate from Docker's raw API objects.

### Utilities

Shared helpers such as table rendering.

---

## Development Workflow

1. Add functionality in the API layer
2. Map API results into models
3. Expose via CLI commands
4. Format output via utilities

---

## Troubleshooting

### Cannot connect to Docker daemon

```
Cannot connect to the Docker daemon
```

Ensure:

* Docker is running
* `DOCKER_HOST` is set correctly
* TCP port is exposed if using remote

---

### Not a swarm manager

```
This node is not a swarm manager
```

Ensure:

* You are connected to a manager node
* Verify with:

```
docker node ls
```

---

### No nodes returned

Check:

```
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

* Node inspection
* Service inspection
* Service scaling
* Service logs
* Stack deployment and removal
* CI/CD integration
* Remote build and deploy pipelines

---

## Contributing

Contributions are welcome. The project is designed to be modular and easy to extend.

---

## License

MIT License
