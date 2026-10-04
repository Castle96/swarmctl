# swarmctl

`swarmctl` is a kubectl-inspired CLI and terminal dashboard for managing Docker
Swarm clusters. It wraps the Docker Engine API with [Bollard](https://github.com/fussybeaver/bollard)
and adds the ergonomics Swarm's own CLI lacks: label and field selectors,
`--sort-by`, JSON/YAML output, declarative apply/diff/patch, and a real TUI.

Built for homelabs, DevOps workflows, and CI/CD pipelines where you want
scriptable control over a Swarm cluster without hand-rolling `curl` against
`/v1.xx`.

## Features

- **Inspect** — nodes, services, tasks, networks, secrets, configs, stacks, volumes
- **Select and filter** — `--selector` (labels), `--field-selector`, `--sort-by`, `-A`
- **Output** — `table` (default), `json`, `yaml`, `wide`, `name`
- **Declarative** — `apply`, `replace`, `patch`, `diff`, `edit`
- **Operate** — `scale`, `rollout`, `exec`, `attach`, `logs`, `cp`, `port-forward`, `top`, `events`, `wait`
- **Node lifecycle** — `cordon`, `uncordon`, `drain`, `taint`, `promote`, `demote`
- **Swarm lifecycle** — `init`, `join`, `leave`, `token`, `status`
- **Multi-host** — Docker contexts, `ssh://` transports, and `discover` to scan a subnet and join a swarm
- **Local vault** — encrypted at rest with Argon2id + AES-256-GCM for join tokens
- **Failover** — container failover policy and volume data migration
- **Interactive TUI** — real-time dashboard and port visualizer
- **Shell completions** — bash, zsh, fish, PowerShell

## Quick start

```bash
git clone https://github.com/Castle96/swarmctl
cd swarmctl
cargo build --release

./target/release/swarmctl --help
```

See [BUILD.md](./BUILD.md) for prerequisites, cross-compilation, and
installation options.

### First commands

```bash
swarmctl cluster-info                # is this node a manager, and what state is the swarm in?
swarmctl get nodes                   # table of nodes
swarmctl get services -o json        # machine-readable
swarmctl describe services web       # full detail
swarmctl dashboard                   # interactive TUI
```

## Command surface

| Group | Commands |
|---|---|
| Read | `get`, `describe`, `explain`, `api-resources`, `top`, `logs`, `events`, `ports`, `cluster-info`, `system` |
| Write | `create`, `apply`, `replace`, `patch`, `edit`, `delete`, `scale`, `run` |
| Operate | `exec`, `attach`, `cp`, `port-forward`, `rollout`, `wait` |
| Nodes | `cordon`, `uncordon`, `drain`, `taint`, `label`, `set`, `promote`, `demote` |
| Swarm | `swarm init\|join\|leave\|token\|status` |
| Cluster config | `stack`, `config`, `volume`, `context`, `vault`, `failover` |
| Meta | `version`, `completion`, `help` |

### Resource types

`get` and `describe` accept these, with short names:

| Resource | Short |
|---|---|
| `nodes` | `no` |
| `services` | `svc` |
| `tasks` | `po` |
| `networks` | `net` |
| `secrets` | `sec` |
| `configs` | `cm` |
| `stacks` | `st` |

## Usage

### Selecting and sorting

```bash
swarmctl get services --selector app=web         # label selector
swarmctl get nodes --field-selector availability=active
swarmctl get services --sort-by .replicas        # numeric fields sort numerically
swarmctl get services --watch                    # re-render every 2s
swarmctl get services --show-labels
```

`--field-selector` and `--sort-by` address fields of the **rendered row**, not
the raw Docker API object — the same fields shown in table output. Run
`swarmctl explain <resource>` for the list per resource type. Paths may be
written with or without a leading dot (`.replicas` and `replicas` are
equivalent).

`--watch` and `--output` are global, so they work before or after the
subcommand: `swarmctl --watch get services` and `swarmctl get services --watch`
are equivalent.

### Output formats

```bash
swarmctl get services -o table   # default; aligned columns
swarmctl get services -o json
swarmctl get services -o yaml
swarmctl get services -o wide    # extra columns
swarmctl get services -o name    # bare resource names, for piping
```

### Stacks

```bash
swarmctl stack deploy -c docker-compose.yml my-stack
swarmctl stack ls
swarmctl stack rm my-stack
```

### Connecting to a remote host

Resolution order: `--context` → `--host` / `$DOCKER_HOST` → active Docker
context → local socket.

```bash
# Local socket (default)
swarmctl get nodes

# Remote over TCP
swarmctl --host tcp://10.0.0.5:2376 get nodes

# Remote over SSH — no TLS setup required
swarmctl --host ssh://user@10.0.0.5 get nodes

# Named context
swarmctl context create prod --host tcp://10.0.0.5:2376
swarmctl context use prod
swarmctl get nodes
```

### TLS

Pass certificate paths explicitly, or let the standard Docker environment
variables drive it:

```bash
swarmctl --host tcp://10.0.0.5:2376 \
         --tlscacert ~/.docker/ca.pem \
         --tlscert  ~/.docker/cert.pem \
         --tlskey   ~/.docker/key.pem \
         get nodes

# Or:
export DOCKER_HOST=tcp://10.0.0.5:2376
export DOCKER_TLS_VERIFY=1
export DOCKER_CERT_PATH=~/.docker
swarmctl get nodes
```

### Local vault

Swarm join tokens grant manager access. `swarmctl` can store them in an
encrypted local file (`~/.swarmctl/vault.json`) rather than leaving them in
shell history.

```bash
swarmctl vault init        # prompts for a password; derives a key with Argon2id
swarmctl vault status
swarmctl vault unlock
swarmctl vault set-key     # rotate the password
```

The vault is encrypted with AES-256-GCM using a key derived from your password
via Argon2id, with a fresh random salt and nonce per write. There is no recovery
path if the password is lost.

### Network discovery

Scan a subnet for Docker daemons and swarm nodes, then optionally join:

```bash
swarmctl discover                                  # detect and scan the local subnet
swarmctl discover --subnet 192.168.1.0/24          # explicit CIDR (/16–/30)
swarmctl discover --json                           # scan only, no prompts
swarmctl discover --subnet 192.168.1.0/24 --no-tui # plain-text prompts
```

Discovery probes the Docker API port and the swarm port on each address, then
offers to join as worker or manager. Prefix length is restricted to `/16`–`/30`
to bound the scan.

### Failover and volume migration

```bash
swarmctl failover status
swarmctl failover enable
swarmctl failover disable
swarmctl failover migrate web-data --from node1 --to node2
```

### Interactive TUI

```bash
swarmctl dashboard        # cluster overview
swarmctl ports --tui      # port allocation visualizer
```

Dashboard keys: `Tab` switch view, `1`–`6` jump to a view, `j`/`k` scroll,
`r` refresh, `q` quit. The dashboard starts in offline mode if no Docker daemon
is reachable.

### Shell completions

```bash
swarmctl completion bash > /etc/bash_completion.d/swarmctl
swarmctl completion zsh  > "${fpath[1]}/_swarmctl"
swarmctl completion fish > ~/.config/fish/completions/swarmctl.fish
swarmctl completion powershell | Out-String | Invoke-Expression
```

## Configuration

| Variable | Purpose | Default |
|---|---|---|
| `DOCKER_HOST` | Daemon address | local socket |
| `DOCKER_CONTEXT` | Context to use | current context |
| `DOCKER_TLS_VERIFY` | Set to `1` to enable TLS | unset |
| `DOCKER_CERT_PATH` | Directory holding `ca.pem`/`cert.pem`/`key.pem` | `~/.docker` |
| `RUST_LOG` | Log level (`error`, `warn`, `info`, `debug`, `trace`) | `info` |

Resolution order is `--context`, then `--host`/`$DOCKER_HOST`, then the active
Docker context, then the local socket. `$XDG_CONFIG_HOME` and platform home
directories are respected when locating Docker's config.

## Architecture

```
src/
├── main.rs          entry point
├── api/             Docker Engine API layer (one module per concern)
│   ├── client.rs    connection + TLS/ssh/context resolution
│   ├── node.rs service.rs task.rs network.rs secret.rs config.rs stack.rs
│   ├── swarm.rs     swarm lifecycle (init/join/leave/token)
│   ├── volume.rs    volume operations
│   ├── discovery.rs subnet scanning
│   ├── failover.rs  failover policy + events
│   ├── migration.rs volume data migration
│   ├── context.rs   Docker context read/write
│   └── docker_check.rs  daemon reachability and config paths
├── cli/             clap command definitions and handlers
├── models/          display-oriented row structs (tabled + serde)
├── tui/             ratatui dashboard and port visualizer
├── vault/           encrypted local token storage
└── utils/           table/JSON/YAML printing, label selectors
```

The layers are deliberately separate: `api/` knows about Bollard types,
`models/` knows about presentation, and `cli/`/`tui/` are the only things that
print. Swapping the TUI or adding an output format does not touch the API layer.

## Development

```bash
make build          # debug build + install to ~/.local/bin
make build-release  # release build + install
make test           # unit tests
make fmt            # format
make clippy         # lint
make check          # cargo check
make ci             # release build + tests
```

CI (`.github/workflows/ci.yml`) runs on every push and pull request and
enforces `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, and a
`--no-default-features` build to keep the non-TUI configuration compiling.

The TUI is behind the default `tui` feature, so `--no-default-features` yields a
smaller binary without `ratatui`/`crossterm`.

### Layout conventions

- One module per Docker resource under `api/`, thin handlers under `cli/`.
- Pure logic (selectors, parsers, crypto, CIDR expansion) lives in functions
  free of I/O so it can be unit tested; see the `#[cfg(test)]` modules.
- Cluster-wide flags that apply to several commands are bundled into a
  `*Options` struct (`get::ListOptions`, `delete::DeleteOptions`,
  `logs::LogOptions`) rather than passed as long positional parameter lists.

## Troubleshooting

**`Cannot connect to the Docker daemon`**
Docker is not running, or `DOCKER_HOST` points somewhere unreachable. Check with
`docker info`, then `swarmctl system ping`. `swarmctl system info` prints the
resolved connection target.

**`This node is not a swarm manager`**
Most operations require a manager. Verify with `swarmctl swarm status` and
`docker node ls`.

**`No nodes returned`**
The node is not in a swarm, or you are connected to a worker. `swarmctl
cluster-info` reports both conditions.

**`error: unexpected argument '--watch'`**
Older builds required the flag before the subcommand. Current builds accept
`--watch` in either position.

## Roadmap

- [x] Resource inspection with selectors and multiple output formats
- [x] Service scaling, logs, and rollout control
- [x] Declarative apply/replace/patch/diff
- [x] Stack deployment from Compose files
- [x] Interactive TUI dashboard and port visualizer
- [x] Swarm lifecycle and node management
- [x] Docker contexts, `ssh://` transport, TLS
- [x] Encrypted local vault for join tokens
- [x] Subnet discovery
- [x] Failover policy and volume migration
- [ ] Integration test suite against a live swarm fixture
- [ ] Release automation and packaged binaries

## Contributing

Modular and easy to extend. Add capability in `api/`, map results into
`models/`, then expose it from `cli/` or `tui/`. Keep pure logic I/O-free so it
stays testable, and make sure `cargo fmt`, `cargo clippy`, and `cargo test` all
pass before opening a pull request.

## License

MIT — see [LICENSE](./LICENSE).