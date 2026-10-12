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
- **Select and filter** — `-l/--selector` (full set-based grammar), `--field-selector`, `--sort-by`
- **Output** — `table` (default), `json`, `yaml`, `wide`, `name`, `jsonpath`, `custom-columns`, `go-template`
- **kubectl-style addressing** — `TYPE/NAME`, comma-separated types, `.VERSION` qualifiers, and a `kind`/`apiVersion`/`metadata`/`spec`/`status` envelope in every machine format
- **Declarative** — `apply`, `replace`, `patch`, `diff`, `edit`
- **Operate** — `scale`, `rollout`, `exec`, `attach`, `logs`, `cp`, `port-forward`, `top`, `events`, `wait`
- **Node lifecycle** — `cordon`, `uncordon`, `drain`, `taint`, `promote`, `demote`
- **Swarm lifecycle** — `init`, `join`, `leave`, `token`, `status`
- **Endpoint registry** — named remote/offsite clusters with site, environment and tags, plus reachability and swarm state at a glance
- **Multi-host** — Docker contexts, `ssh://` transports, and `discover` to scan a subnet and join a swarm
- **Fan-out** — run one command against several endpoints and combine or compare the results
- **Cross-cluster diff** — compare resources between two clusters and gate CI on the result
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

## Managing offsite clusters

An *endpoint* is a named remote Docker daemon, usually a manager node. Docker
contexts can address a remote daemon but cannot record which site or environment
it belongs to, so `swarmctl` keeps its own registry and mirrors it into Docker.

```bash
# Register. ssh:// needs no TLS setup and is the safest option over a VPN.
swarmctl endpoint add prod-eu \
  --host ssh://ops@10.8.0.4 \
  --site eu-west --env prod --tags offsite,vpn

swarmctl endpoint add staging-us \
  --host tcp://10.9.0.4:2376 \
  --tlscacert ~/.docker/ca.pem \
  --tlscert  ~/.docker/cert.pem \
  --tlskey   ~/.docker/key.pem \
  --site us-east --env staging

swarmctl endpoint ls
```

Endpoints are validated on write: the host must carry a supported scheme
(`tcp://`, `ssh://`, `unix://`, `https://`), names cannot contain whitespace or
`/`, and an incomplete TLS triple is rejected rather than failing at connect time.

### Health

```bash
swarmctl endpoint health              # probe everything
swarmctl endpoint health --endpoint prod-eu,staging-us
swarmctl endpoint health --watch      # re-probe every 5s
```

```
name       env      site     status       swarm           nodes     version rtt
lab        lab      local    no-swarm     -               1         29.7    1ms
prod-eu    prod     eu-west  ok           prod            12 (3 mgr) 29.7    34ms
staging-us staging  us-east  unreachable -               -         -       5s
```

Each endpoint gets its own timeout, so one unreachable cluster never stalls the
rest. A node that answers but is not in a swarm reports `no-swarm`, which is
distinct from `unreachable`.

### Fan-out

Run any command against several endpoints:

```bash
swarmctl get nodes --endpoints prod-eu,prod-us,staging-us
swarmctl get services --endpoints default -o wide
swarmctl top stats --endpoints prod-eu,staging-us
```

Read-only commands render one combined result per endpoint. `-o json` emits a
single JSON document keyed by endpoint name, so it stays pipeable:

```json
{ "prod-eu": [ ... ], "prod-us": [ ... ] }
```

Mutating commands list the targets, require confirmation (or `--yes`), and
report per-endpoint results. They exit non-zero if any endpoint failed, but a
failure on one endpoint does not abort the others:

```bash
swarmctl scale web 3 --endpoints prod-eu,prod-us   # asks first
swarmctl scale web 3 --endpoints prod-eu,prod-us --yes
```

`events` and `wait` cannot be fanned out — neither returns on its own.

### Cross-cluster diff

```bash
swarmctl diff-clusters prod-eu prod-us --resource services
swarmctl diff-clusters prod-eu prod-us --resource services -o json
```

```
prod-eu vs prod-us: 2 difference(s)
  ~ service web
      replicas: 3/3 -> 1/3
  + service api
  - service legacy-worker
```

Exits `0` when the clusters match and `1` when they differ or a side was
unreachable, so it can gate a deployment. Tasks are not comparable: their ids
are per-run, so every cluster would look wholly different.

### Drift detection

`apply` records the manifest it submits, and `drift` compares the live cluster
against those records. This is the question Swarm otherwise cannot answer:
whether what is running is still what you asked for.

```bash
swarmctl apply -f web.yml          # records the manifest
swarmctl drift                     # compare live state against the records
swarmctl drift -o json             # machine-readable, for CI
swarmctl drift --endpoints prod-eu,prod-us   # check several clusters at once
```

```
prod-eu: 1 resource(s) drifted
~ service web
      TaskTemplate.ContainerSpec.Image: nginx:1.25 -> nginx:1.27
```

Two things drift reports that a plain `get` cannot:

- **Drifted** — the field no longer matches the manifest. Usually an image tag
  that was repointed by a push, or a replica count edited by hand.
- **Missing** — recorded in the store but gone from the cluster.

Resources running with no recorded manifest are listed as untracked; that is a
fact about the store rather than drift, so it does not affect the exit code.

`drift` exits `0` when every recorded resource matches and `1` when anything has
drifted, so `drift && deploy.sh` gates a pipeline on declared state being true.
It needs no `apply` history to be meaningful — but only resources you have
applied through swarmctl are tracked, so start by re-applying the services you
want watched.

Server-assigned fields (`Version`, `CreatedAt`, `UpdateStatus`, `ForceUpdate`,
node bookkeeping) are never reported: no manifest reproduces them, so including
them would bury the real change in noise.

To stop tracking, `swarmctl drift forget` clears the current endpoint's records
and `swarmctl drift forget --all` clears every endpoint's. Retiring a cluster
otherwise leaves `drift` reporting its resources as missing forever.

```bash
swarmctl drift forget              # this endpoint
swarmctl drift forget prod-eu      # one named endpoint
swarmctl drift forget --all        # every endpoint with recorded state
```


### Docker interoperability

`endpoint add --sync` writes a real Docker context (directory `sha256(name)`,
TLS material under `contexts/tls/<digest>/docker/`), so a swarmctl-managed
endpoint works with Docker directly:

```bash
docker context ls                # shows swarmctl-managed endpoints
docker --context prod-eu info
```

The reverse direction works too:

```bash
swarmctl endpoint import         # adopt existing Docker contexts
swarmctl endpoint sync           # (re)write Docker contexts for all endpoints
swarmctl endpoint use prod-eu    # make it the default, for Docker and swarmctl
```

### Credentials per cluster

The vault stores join tokens per endpoint rather than for a single cluster.
See [Local vault](#local-vault).

## Command surface

| Group | Commands |
|---|---|
| Read | `get`, `describe`, `explain`, `api-resources`, `top`, `logs`, `events`, `ports`, `cluster-info`, `system`, `drift` |
| Write | `create`, `apply`, `replace`, `patch`, `edit`, `delete`, `scale`, `run` |
| Operate | `exec`, `attach`, `cp`, `port-forward`, `rollout`, `wait` |
| Nodes | `cordon`, `uncordon`, `drain`, `taint`, `label`, `set`, `promote`, `demote` |
| Swarm | `swarm init\|join\|leave\|token\|status` |
| Cluster config | `stack`, `config`, `volume`, `context`, `vault`, `failover` |
| Endpoints | `endpoint` (`add`/`ls`/`rm`/`inspect`/`health`/`import`/`use`/`sync`) |
| Multi-cluster | `diff-clusters`, `drift`, plus `--endpoints` on any command |
| Meta | `version`, `completion`, `help` |

### Global flags

Accepted before or after the subcommand.

| Flag | Purpose |
|---|---|
| `-o, --output` | `table` (default), `wide`, `json`, `yaml`, `name`, `jsonpath=EXPR`, `jsonpath-as-json=EXPR`, `go-template=EXPR`, `custom-columns=SPEC`. Machine formats suppress the connection banner so output stays pipeable. |
| `-w, --watch` | Re-render periodically. Only `get` acts on it. |
| `--host` | Daemon address; also read from `$DOCKER_HOST`. |
| `--context` | Docker context; also read from `$DOCKER_CONTEXT`. **Conflicts with `--host`** rather than silently overriding it. |
| `--tlscacert`, `--tlscert`, `--tlskey` | TLS material. All three are required together. Ignored with a warning when `--context` is also set, because a context carries its own TLS settings. |
| `--endpoints` | Run against named endpoints. See [Fan-out](#fan-out). |
| `-y, --yes` | Assume yes for write fan-out. Required in non-interactive use. |
| `-v` | Verbosity (repeatable). |

### Per-command flags worth knowing

| Command | Flag | Purpose |
|---|---|---|
| `get` | `-l, --selector` | Label selector: `key=value`, `key==v`, `key!=v`, `key`, `!key`, `key in (a,b)`, `key notin (a,b)`. Comma-separated terms are ANDed. |
| `get` | `--field-selector` | Field selector against the object envelope (same paths as `--sort-by` and `-o jsonpath`) |
| `get` | `--sort-by` | Order by a jsonpath expression; numerics sort numerically, ties keep input order |
| `get` | `-L, --label-columns` | Append label values as extra columns |
| `get` | `--no-headers` | Omit the table header |
| `get` | `--show-kind` | Prefix `-o name` output with the resource type |
| `get` | `--show-labels` | Print labels after each row |
| `get` | `TYPE/NAME` | Address one object inline: `get svc/mcp_fetch` |
| `delete` | `--dry-run` | Report what would be deleted |
| `delete` | `--selector` | Delete everything matching a label |
| `delete` | `--ignore-not-found` | Exit 0 when already absent. Does not apply to `delete tasks`, which is unsupported outright. |
| `delete` | `--grace-period` | Container shutdown grace period, applied to the service before removal |
| `delete` | `--wait`, `--timeout` | Block until the resource is really gone (default bound 60s) |
| `logs` | `--follow` | Stream |
| `logs` | `--tail` | Lines of history; `0` means all |
| `logs` | `--timestamps` | Prefix each line with its timestamp |
| `logs` | `--previous` | Read the previous (crashed) task's logs |
| `logs` | `--prefix` | Prefix each line with the resource name |
| `logs` | `--ignore-errors` | Continue past per-line read errors |
| `ports` | `--available` | Show free ports in a range |
| `ports` | `--range-start`, `--range-end` | Range to scan (default 30000-40000) |
| `ports` | `--protocol` | `tcp` or `udp` |
| `patch` | `--patch` | Inline JSON, or `@file` to read from a file |
| `patch` | `--patch-type` | Only `merge` (RFC 7386) |
| `patch` | `--dry-run` | Show the resulting spec without applying |
| `run` | `--publish` | `container`, `host:container`, either with `/proto` |
| `run` | `--network` | Attach to an existing network by name |
| `run` | `--env`, `--labels` | `KEY=VALUE`; malformed entries are rejected |
| `wait` | `--condition` | Per-resource; an unknown condition is an error |
| `wait` | `--timeout` | Seconds to wait |
| `events` | `--filter-type` | Filter by event type |
| `label` | `--overwrite` | Replace all labels instead of merging |
| `label` | `--all` | Remove every label |
| `taint` | `--remove` | Remove a taint by key |
| `taint` | `--overwrite` | Replace all taints |
| `diff-clusters` | `--resource` | `nodes`, `services`, `networks`, `secrets`, `configs`, `stacks` |

Run `swarmctl <command> --help` for the full list. swarmctl has no flag
that parses and then does nothing: options it cannot honour are rejected with
an explanation rather than accepted and ignored.

### Declarative management

`apply` makes a resource match a file: it creates the resource if absent and
updates it in place if present. This holds for services, configs and secrets.

Networks are the exception. The Docker API cannot update a network's spec in
place, so `apply networks` creates a missing network and leaves an existing one
untouched, saying so on both stdout and stderr rather than reporting success.
`replace networks --force` does delete and re-create, which orphans the
containers attached to it.

`replace` and `edit` update configs and secrets in place. They previously deleted
the old object and created a new one, so a failed create lost the resource
outright.

### Deleting

`swarmctl delete` removes the resource. Two behaviours are worth knowing:

- `delete services --wait` blocks until the service is really gone. Docker
  returns from the delete call before teardown finishes, so a script that deletes
  and immediately re-creates otherwise races it.
- `delete nodes` **drains** rather than removes. Removing a node from a swarm is
  not exposed by the Docker API client in use, and draining is the safe
  equivalent — new tasks stop being scheduled while running tasks finish. The
  output says `drained`, not `deleted`. Use the Docker CLI if the node itself
  must be removed.

### Ports

`swarmctl ports` reports the same rows in every output format — the table and
`-o json` used to come from different listings, so they disagreed on both row
count and field names for the same cluster. `--protocol` applies to all formats,
and `--available` recomputes against the same filtered set so the free ports
cannot contradict the table above them.

`--range-start` greater than `--range-end` is rejected before any API call;
previously it produced an empty range and a cheerful "No available ports".

### Resource types

Every spelling in a row below resolves to the same resource, so `get nodes`,
`get node`, `get Node` and `get no` are equivalent.

| Resource | Short | Kind | Short names |
|---|---|---|---|
| `nodes` | `no` | `Node` | `no` |
| `services` | `svc` | `Service` | `svc` |
| `tasks` | `po` | `Task` | `po` |
| `networks` | `net` | `Network` | `net` |
| `secrets` | `sec` | `Secret` | `sec` |
| `configs` | `cm` | `Config` | `cm` |
| `stacks` | `st` | `Stack` | `st` |

`swarmctl explain` with no argument prints this table with every accepted
spelling; `swarmctl api-resources` prints it with the API version negotiated
against the connected daemon.

The kubectl argument grammar applies:

```bash
swarmctl get svc                  # one type, all objects
swarmctl get svc,no               # comma-separated types
swarmctl get svc/mcp_fetch        # TYPE/NAME
swarmctl get svc web api          # names as positional arguments
swarmctl get services.v1.52       # .VERSION qualifier, checked against the API version
swarmctl get services.v1.apps     # error: Swarm has no API groups
swarmctl get servics              # "Did you mean this? services"
```

## Usage

### Selecting and sorting

```bash
swarmctl get services -l app=web                        # equality
swarmctl get services -l 'app!=web'                     # not-equal
swarmctl get nodes    -l role                           # key exists
swarmctl get nodes    -l '!role'                        # key does not exist
swarmctl get nodes    -l 'role in (worker,edge)'        # set membership
swarmctl get nodes    -l 'role notin (worker)'          # set exclusion
swarmctl get nodes    -l 'role=edge,zone=prod'          # comma = AND
swarmctl get nodes --field-selector spec.availability=active
swarmctl get nodes --sort-by '{.metadata.name}'
swarmctl get services --watch                           # re-render every 2s
swarmctl get services --show-labels
```

`--selector`, `--field-selector` and `--sort-by` all address the **same object
envelope** described in [Object envelope](#object-envelope), so one set of path
names works for all three. Run `swarmctl explain <resource>` for the documented
paths per resource type.

A malformed selector is an **error**, reported before any API call:

```
$ swarmctl get nodes -l 'role in nope'
Error: --selector: set operator in "role in nope" needs parentheses, e.g. key in (a,b)
```

This used to match zero rows and look exactly like an empty cluster.

`--watch` and `--output` are global, so they work before or after the
subcommand: `swarmctl --watch get services` and `swarmctl get services --watch`
are equivalent.

`--since` (on `logs` and `events`) accepts an absolute Unix timestamp or a
relative duration — `30s`, `5m`, `2h`, `1d`, `1w`, or a compound `1h30m`. A value
with no unit is a timestamp, matching Docker: write `30s`, not `30`, for "the
last 30 seconds".

### Output formats

```bash
swarmctl get services -o table   # default; aligned columns
swarmctl get services -o wide    # table plus labels
swarmctl get services -o json
swarmctl get services -o yaml
swarmctl get services -o name    # bare resource names, for piping
swarmctl get services -o name --show-kind

swarmctl get nodes -o jsonpath='{range .items[*]}{.metadata.name}{"\t"}{.status.state}{"\n"}{end}'
swarmctl get nodes -o jsonpath-as-json='{.items[*].metadata.name}'
swarmctl get nodes -o custom-columns='NAME:.metadata.name,ADDR:.status.addr'
swarmctl get nodes -o go-template='{range .items[*]}{.metadata.name}{"\n"}{end}'

swarmctl get nodes -o custom-columns='NAME:.metadata.name' -L role --no-headers
```

### Object envelope

Every machine format serializes a Kubernetes-shaped object rather than
Bollard's raw structs:

```console
$ swarmctl get nodes -o json | jq '.items[0]'
{
  "apiVersion": "v1.52",
  "kind": "Node",
  "metadata": {
    "name": "zev",
    "labels": { "role": "orchestrator" },
    "uid": "zfthk15sat72zfa68krnxjkmj",
    "creationTimestamp": "2026-09-30T17:03:06.32103471Z"
  },
  "spec":   { "availability": "active", "labels": { "role": "orchestrator" } },
  "status": { "state": "ready", "addr": "100.119.129.40" }
}
```

This is what makes `--field-selector metadata.name=zev`, `--sort-by
'{.metadata.name}'` and `-o jsonpath={.metadata.name}` work at all. Raw Docker
keys are Go-style and capitalized (`Spec.Availability`), which is why the
`spec`/`status` bodies are camelCased and the swarm-specific identity is kept
under `dockerId`.

Services additionally expose `metadata.stack`, taken from the
`com.docker.stack.namespace` label — Swarm's closest equivalent to a namespace.

#### jsonpath

The evaluator covers the subset kubectl users actually write: field access,
`[0]` / `[-1]` indexes, `[*]` wildcards, `..name` recursive descent,
`{.a,.b}` unions, `[?(@.field=="x")]` filters, `{range …}{end}` blocks (which
nest), and `{"\t"}` string escapes.

Slices (`[0:2]`) are not supported and are reported as such rather than
silently resolving to nothing.

```bash
swarmctl get services -o jsonpath='{.items[*].spec.taskTemplate.containerSpec.image}'
swarmctl get tasks -o jsonpath='{range .items[*]}{.status.state}{"\n"}{end}' | sort | uniq -c
```

### Stacks

```bash
swarmctl stack deploy -c docker-compose.yml my-stack
swarmctl stack ls
swarmctl stack rm my-stack
```

### Connecting to a remote host

Resolution order: `--context` / `$DOCKER_CONTEXT` → `--host` / `$DOCKER_HOST`
→ active Docker context → `$DOCKER_TLS_VERIFY` + `$DOCKER_CERT_PATH` → local
socket.

`--context` and `--host` together are a **conflict**, not a precedence rule:

```console
$ swarmctl --context prod --host tcp://10.0.0.5:2375 get nodes
Error: cannot combine --context prod with --host: a context already names a daemon. Drop one, or use --host alone.
```

A context used to win silently, discarding `--host` and every `--tls*` flag, so
a command line could name one daemon and swarmctl would connect to another.
An empty `$DOCKER_CONTEXT` is treated as unset.

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
swarmctl vault init                          # prompts for a password
swarmctl vault status                        # which endpoints hold credentials
swarmctl vault ls -o json
swarmctl vault store prod-eu --manager-token "$TOKEN"
swarmctl vault rm staging-us
swarmctl vault unlock
swarmctl vault set-key                       # rotate the password
```

Credentials are keyed by **endpoint name**, so several clusters can be held at
once. The file is `~/.swarmctl/vault.json`, written `0600` inside a `0700`
directory. Vaults written by older versions (a single cluster) migrate
automatically on first open.

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

Resolution order is `--context`, then `$DOCKER_CONTEXT`, then `--host`,
`$DOCKER_HOST`, then the active Docker context, then `$DOCKER_TLS_VERIFY` with
`$DOCKER_CERT_PATH`, then the local socket — matching Docker's own precedence, so
`swarmctl` and `docker` agree on the target.

A flag always beats its own environment variable: `--host` overrides
`DOCKER_HOST`. Combining `--context` with `--host` is rejected rather than
resolved.

## Architecture

```
src/
├── main.rs          entry point
├── api/             Docker Engine API layer (one module per concern)
│   ├── client.rs    connection + TLS/ssh/context resolution
│   ├── endpoints.rs named endpoint registry + Docker context sync
│   ├── health.rs    endpoint reachability probing
│   ├── diff.rs      cross-cluster comparison
│   ├── specdiff.rs  structured field-aware resource comparison
│   ├── applied.rs   last-applied manifest store (drift)
│   ├── node.rs service.rs task.rs network.rs secret.rs config.rs stack.rs
│   ├── swarm.rs     swarm lifecycle (init/join/leave/token)
│   ├── volume.rs    volume operations
│   ├── discovery.rs subnet scanning
│   ├── failover.rs  failover policy + events
│   ├── migration.rs volume data migration
│   ├── context.rs   Docker context read/write
│   └── docker_check.rs  daemon reachability and config paths
├── cli/             clap command definitions and handlers
│   ├── registry.rs  the resource table: names, short names, kind, grammar
│   ├── kobject.rs   Docker object -> kind/apiVersion/metadata/spec/status
│   ├── rows.rs      shared API-object -> display-row builders
│   ├── fanout.rs    running one command against several endpoints
│   └── drift.rs     drift detection and `drift forget`
├── models/          display-oriented row structs (tabled + serde)
├── tui/             ratatui dashboard and port visualizer
├── vault/           encrypted local token storage, keyed by endpoint
└── utils/           printing, jsonpath, selectors, state paths
```

The layers are deliberately separate: `api/` knows about Bollard types,
`models/` knows about presentation, and `cli/`/`tui/` are the only things that
print. Swapping the TUI or adding an output format does not touch the API layer.

Two invariants are load-bearing and worth stating outright:

- **`cli::registry` is the only place a resource is named.** Short names,
  singulars, `kind`, `TYPE/NAME`, comma lists and `.VERSION` qualifiers all
  resolve through it, so `explain`, `api-resources` and the parser cannot
  disagree about what exists.
- **`cli::kobject` is the only object serializer.** Every machine format, and
  every `--field-selector` and `--sort-by` path, reads the same envelope.

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
  `logs::LogOptions`, `endpoint::AddOptions`) rather than passed as long
  positional parameter lists.
- Resource lookup goes through `api::*::find_*`; row construction through
  `cli::rows`; duration parsing through `utils::time`. Each has one
  implementation so behaviour cannot drift between commands.
- A list handler filters **typed** objects and keeps the serialized form in
  step (`get::Filtered`), so display rows and rendered envelopes can never
  describe different sets. Filtering the serialized side and then building
  rows from the original list is the bug this replaced: `--field-selector`
  worked for `-o json` and did nothing for `-o table`.

## Troubleshooting

**`Cannot connect to the Docker daemon`**
Docker is not running, or the resolved target is unreachable. Check with
`docker info`, then `swarmctl system ping`. `swarmctl system info` prints the
resolved connection target.

**`cannot combine --context X with --host`**
Both were supplied. A context already names a daemon, so drop one of them.

**`the server doesn't have a resource type "servics"`**
The spelling did not resolve. swarmctl suggests the nearest accepted spelling;
`swarmctl explain` lists every spelling of every resource.

**`--selector: set operator ... needs parentheses`**
Set-based selectors need `key in (a,b)`, not `key in a,b`.

**Reaching a cluster over a VPN/tailnet**
`--host ssh://user@host` needs no TLS setup and is the safest option. It does
require SSH access, and opens a connection per call, so it is slower than a
native TCP endpoint. Note that a swarm node may expose the raft port (2377)
while leaving the Engine API (2375/2376) closed — swarmctl needs the Engine
API, so a reachable 2377 does not mean `swarmctl` can talk to it.

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
- [x] Endpoint registry with site/environment metadata and Docker interop
- [x] Endpoint health checks
- [x] Read and write fan-out across endpoints
- [x] Cross-cluster diff, with a CI-gateable exit code
- [x] Drift detection: last-applied manifests, `drift`, `drift forget`
- [x] Per-cluster credentials in the vault
- [x] kubectl argument grammar: `TYPE/NAME`, comma lists, `.VERSION`, singular and `kind` spellings, typo suggestions
- [x] Kubernetes-shaped object envelope shared by every machine format and path expression
- [x] jsonpath and `custom-columns` output, `-L`, `--no-headers`, `--show-kind`
- [x] Full set-based selector grammar with errors on malformed input
- [x] `api-resources` probes the connected daemon instead of printing a constant
- [ ] Integration test suite against a live swarm fixture
- [ ] Drift tracking for configs, secrets, and networks (services only today)
- [ ] Volume data migration (needs filesystem access to each node)
- [ ] Release automation and packaged binaries

## Contributing

Modular and easy to extend. Add capability in `api/`, map results into
`models/`, then expose it from `cli/` or `tui/`. Keep pure logic I/O-free so it
stays testable, and make sure `cargo fmt`, `cargo clippy`, and `cargo test` all
pass before opening a pull request.

## License

MIT — see [LICENSE](./LICENSE).
