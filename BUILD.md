# Building and running swarmctl

## Prerequisites

1. **Rust** (stable, 1.85 or newer — the crate uses edition 2024)

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source ~/.cargo/env
   ```

2. **Build tools**

   - Ubuntu/Debian: `sudo apt install build-essential`
   - Fedora/RHEL: `sudo dnf install gcc`
   - macOS: `xcode-select --install`

3. **Docker**, to talk to a swarm (only needed at run time, not to build)

   - A reachable Docker daemon, via the local socket, `tcp://`, or `ssh://`
   - A **manager** node for cluster-wide operations

## Building

```bash
git clone https://github.com/Castle96/swarmctl
cd swarmctl

cargo build              # debug
cargo build --release    # optimized
```

Binaries land in `target/debug/swarmctl` and `target/release/swarmctl`.

### Feature flags

The interactive dashboard and port visualizer sit behind the default `tui`
feature:

```bash
cargo build --release                        # with TUI (default)
cargo build --release --no-default-features  # without ratatui/crossterm
```

Both configurations are built and tested in CI, so `--no-default-features` is
expected to keep working.

### Using Docker to build

```bash
docker run --rm -v "$(pwd):/app" -w /app rust:latest cargo build --release
```

Note this bind-mounts the source but not a Cargo cache, so it re-downloads and
recompiles all dependencies. A named volume for `target/` and
`$CARGO_HOME` makes repeat builds much faster.

### Cross-compilation

The crate is pure Rust with no C dependencies beyond the standard toolchain, so
cross-compiling only needs the target's linker:

```bash
rustup target add aarch64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu

rustup target add x86_64-pc-windows-msvc   # or use --gnu with mingw
cargo build --release --target x86_64-pc-windows-msvc
```

## Installing

```bash
make build-release          # builds, then copies to ~/.local/bin/swarmctl

# or manually
cargo install --path .

# or system-wide
sudo cp target/release/swarmctl /usr/local/bin/
```

Make sure `~/.local/bin` is on your `PATH`.

## Verification

```bash
cargo fmt --all -- --check           # formatting
cargo clippy --all-targets -- -D warnings
cargo test                           # unit tests
cargo build --no-default-features    # non-TUI configuration
```

`.github/workflows/ci.yml` runs all four on every push and pull request, and
treats warnings as errors. Run them locally before opening a PR.

## Usage

```bash
swarmctl --help              # top-level commands
swarmctl <command> --help    # per-command flags
swarmctl explain services    # fields available to --field-selector / --sort-by
```

Common commands:

| Command | Description |
|---|---|
| `get` | Display one or many resources |
| `describe` | Show detailed information about a resource |
| `apply` / `replace` / `patch` / `diff` / `edit` | Declarative management |
| `create` / `delete` | Imperative management |
| `scale` / `rollout` | Change service replica counts |
| `logs` / `exec` / `attach` / `cp` | Container interaction |
| `cordon` / `uncordon` / `drain` / `taint` | Node scheduling control |
| `swarm init\|join\|leave\|token\|status` | Swarm lifecycle |
| `stack deploy\|ls\|rm` | Compose-based stack management |
| `context` | Manage Docker contexts |
| `vault` | Encrypted local storage for join tokens |
| `discover` | Scan a subnet for Docker hosts and swarms |
| `failover` / `volume` | Failover policy and volume migration |
| `ports` | Port mapping inventory and availability |
| `top` / `events` / `wait` | Observability |
| `system info\|df\|ping` | Daemon-level information |
| `dashboard` | Interactive TUI dashboard |
| `completion <shell>` | Shell completions |

Resource types accepted by `get`, `describe`, and friends: `nodes` (`no`),
`services` (`svc`), `tasks` (`po`), `networks` (`net`), `secrets` (`sec`),
`configs` (`cm`), `stacks` (`st`).

## Configuration

| Variable | Purpose | Default |
|---|---|---|
| `DOCKER_HOST` | Daemon address | local socket |
| `DOCKER_CONTEXT` | Context to use | current Docker context |
| `DOCKER_TLS_VERIFY` | `1` enables TLS | unset |
| `DOCKER_CERT_PATH` | Directory with `ca.pem`, `cert.pem`, `key.pem` | `~/.docker` |
| `RUST_LOG` | Log level | `info` |

Resolution order: `--context`, then `--host` / `$DOCKER_HOST`, then the active
Docker context, then the local socket.

```bash
export DOCKER_HOST=ssh://user@swarm-manager.example.com
export RUST_LOG=debug
swarmctl get nodes
```

## Connecting to a remote swarm

### Over SSH (recommended — no TLS to configure)

Docker's API is exposed on the remote host's socket, which SSH already protects
with its own authentication and transport encryption:

```bash
swarmctl --host ssh://user@swarm-manager.example.com get nodes
```

### Over TCP with TLS

Generate certificates with [dockerd-gen-certs](https://github.com/docker/docker-py/blob/master/docs/user-guide.rst#docker-contexts-tls),
then point `swarmctl` at them:

```bash
swarmctl --host tcp://swarm-manager.example.com:2376 \
         --tlscacert ~/.docker/ca.pem \
         --tlscert  ~/.docker/cert.pem \
         --tlskey   ~/.docker/key.pem \
         get nodes
```

Or via environment variables:

```bash
export DOCKER_HOST=tcp://swarm-manager.example.com:2376
export DOCKER_TLS_VERIFY=1
export DOCKER_CERT_PATH=~/.docker
swarmctl get nodes
```

### Over plain TCP

Only appropriate on a trusted private network or a VPN/tailscale interface, and
only for throwaway environments.

> **Warning:** the Docker API grants full control of the host, including
> mounting the root filesystem. Plain TCP on port 2375 is unauthenticated:
> anything that can reach the port owns the machine. Never bind it to
> `0.0.0.0` on a network you do not control, and never expose it to the
> internet.

If you must, bind to a specific private interface rather than all interfaces,
and firewall the port to known peers:

```ini
# /etc/systemd/system/docker.service.d/override.conf
[Service]
ExecStart=
ExecStart=/usr/bin/dockerd -H unix:///var/run/docker.sock -H tcp://10.0.0.5:2375
```

```bash
sudo systemctl daemon-reload
sudo systemctl restart docker
ss -lntp | grep dockerd
```

Prefer SSH or TLS for anything you care about.

## Troubleshooting

**`Cannot connect to the Docker daemon`**
The daemon is not running, or the address is wrong. Check `docker info`, then
`swarmctl system ping`. `swarmctl system info` prints the connection target
`swarmctl` resolved and where it came from.

**`Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the
daemon running?` as a non-root user**
Add yourself to the `docker` group, then log out and back in:

```bash
sudo usermod -aG docker "$USER"
```

**`503: This node is not a swarm manager`**
Cluster operations need a manager. Check with `swarmctl swarm status`. On a
fresh node, `swarmctl swarm init` creates a single-node swarm.

**Build fails on `edition2024`**
Your toolchain predates Rust 1.85. Run `rustup update stable`.

**Build fails in a linker**
Install the platform build tools listed under Prerequisites.

**`error: unexpected argument '--watch'` on `get`**
Older builds required the flag before the subcommand
(`swarmctl --watch get services`). Current builds accept either position.

## Contributing

1. Add capability in `api/` against the Bollard client.
2. Map results into display structs in `models/`.
3. Expose it from `cli/` (and `tui/` if it warrants a view).
4. Keep pure logic free of I/O so it can be unit tested.
5. Verify: `cargo fmt`, `cargo clippy --all-targets`, `cargo test`.

## License

MIT — see [LICENSE](./LICENSE).