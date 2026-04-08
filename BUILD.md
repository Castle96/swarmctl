# Building and Running swarmctl

## Prerequisites

### Required Software

1. **Rust** (latest stable)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source ~/.cargo/env
   ```

2. **Build Tools**
   - **Ubuntu/Debian:**
     ```bash
     sudo apt update && sudo apt install build-essential
     ```
   - **Fedora/RHEL:**
     ```bash
     sudo dnf install gcc
     ```
   - **macOS:**
     ```bash
     xcode-select --install
     ```

3. **Docker** (for connecting to Docker Swarm)
   - Docker daemon must be running
   - Access to Docker socket or TCP endpoint

## Building

### Native Build (Recommended)

```bash
# Clone repository
git clone <repository-url>
cd swarmctl

# Build debug version
cargo build

# Build release version (optimized)
cargo build --release

# Build with specific features
cargo build --release --features <feature>
```

### Using Docker

```bash
# Build inside Docker container
docker run --rm -v $(pwd):/app -w /app rust:latest cargo build --release

# Build with specific Rust version
docker run --rm -v $(pwd):/app -w /app rust:1.75 cargo build --release
```

## Running

### Binary Location

After building:
- Debug: `./target/debug/swarmctl`
- Release: `./target/release/swarmctl`

### Installation (Optional)

```bash
# Copy to system PATH
sudo cp ./target/release/swarmctl /usr/local/bin/swarmctl

# Or install to user PATH
mkdir -p ~/.local/bin
cp ./target/release/swarmctl ~/.local/bin/
export PATH="$HOME/.local/bin:$PATH"
```

## Usage

### View Help

```bash
./target/release/swarmctl --help
./target/release/swarmctl <command> --help
```

### Available Commands

| Command | Description |
|---------|-------------|
| `get` | Display one or many resources |
| `describe` | Show detailed information about a resource |
| `create` | Create a resource from a file or stdin |
| `delete` | Delete resources |
| `scale` | Scale a service |
| `logs` | Fetch the logs of a resource |
| `ports` | List and visualize port mappings |
| `cluster-info` | Get cluster information |
| `dashboard` | Launch interactive TUI dashboard |
| `version` | Show version information |

### Resource Types

```bash
swarmctl get nodes
swarmctl get services
swarmctl get tasks
swarmctl get networks
swarmctl get secrets
swarmctl get configs
swarmctl get stacks
```

### Output Formats

```bash
# Table format (default)
swarmctl get services

# JSON format
swarmctl get services -o json

# YAML format
swarmctl get services -o yaml
```

## Port Scanner

The port scanner helps you manage container port mappings.

```bash
# List all port mappings
swarmctl ports

# TUI visualization
swarmctl ports --tui

# Show available ports in range
swarmctl ports --available

# Filter by protocol
swarmctl ports --protocol tcp
swarmctl ports --protocol udp

# Custom port range
swarmctl ports --range-start 30000 --range-end 40000

# Combined options
swarmctl ports --available --protocol tcp --range-start 8000 --range-end 9000
```

## TUI Dashboard

Interactive terminal dashboard with real-time monitoring.

```bash
# Launch dashboard
swarmctl dashboard
```

### Dashboard Controls

| Key | Action |
|-----|--------|
| `Tab` | Switch between views |
| `1` | Go to Services |
| `2` | Go to Nodes |
| `3` | Go to Networks |
| `4` | Go to Ports |
| `5` | Go to Secrets |
| `6` | Go to Tasks |
| `j` / `Down` | Navigate down |
| `k` / `Up` | Navigate up |
| `r` | Refresh data |
| `q` | Quit |

## Examples

### Basic Operations

```bash
# List all services
swarmctl get services

# Get specific service
swarmctl get services my-service

# Describe a service
swarmctl describe services my-service

# Scale a service
swarmctl scale my-service 5

# View logs
swarmctl logs service my-service
swarmctl logs service my-service --follow

# Cluster information
swarmctl cluster-info
```

### Filtering and Selection

```bash
# Show labels
swarmctl get services --show-labels

# Filter by label
swarmctl get services --selector app=web

# Watch for changes
swarmctl get services --watch
```

## Troubleshooting

### Connection Issues

```bash
# Check if Docker is running
docker info

# Set Docker host explicitly
export DOCKER_HOST=tcp://localhost:2375

# Or use Unix socket
export DOCKER_HOST=unix:///var/run/docker.sock
```

### Build Issues

```bash
# Update Rust
rustup update

# Clean and rebuild
cargo clean
cargo build --release

# Check for errors
cargo build --release 2>&1 | head -50
```

### Permission Issues

```bash
# Add user to docker group (Linux)
sudo usermod -aG docker $USER
# Log out and back in for changes to take effect

# Or run with Docker socket access
docker run --rm -v /var/run/docker.sock:/var/run/docker.sock ./target/release/swarmctl get services
```

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `DOCKER_HOST` | Docker daemon address | `unix:///var/run/docker.sock` |
| `RUST_LOG` | Logging level | `info` |

Example:
```bash
export DOCKER_HOST=tcp://192.168.1.100:2375
export RUST_LOG=debug
./swarmctl get services
```

## Contributing

When making changes:

1. Make your changes
2. Build to verify: `cargo build --release`
3. Test locally
4. Run tests: `cargo test`
5. Commit changes

## License

See LICENSE file for details.
