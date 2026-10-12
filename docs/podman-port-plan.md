# Porting swarmctl to Podman: a plan

Planning document only. Nothing here is implemented. The question is what
happens to swarmctl if Docker becomes obsolete, and the honest answer is that
the answer depends on which thing obsoletes first: the *daemon* or the
*orchestrator*.

---

## 0. The premise, checked

"Docker becomes obsolete" splits into two independent risks, and they need
different responses.

**Swarm mode is not dead, but it is stagnant.** Docker's own deprecated-features
page lists classic Swarm and overlay networks with an external KV store as
removed, while Swarm *mode* remains in every current Engine. SwarmKit (the
library behind it) merged pull requests in August 2026 including CSI volume
work, and Mirantis — which acquired Docker Enterprise — states it maintains,
tests, and supports Swarm inside Mirantis Kubernetes Engine with a dedicated
team and 24x7 subscriptions. Docker Engine 29.8.0 shipped seven Swarm-specific
fixes. So: no end-of-life date, active fixes, but no momentum either.

**Podman has no Swarm equivalent at all.** This is the load-bearing fact. Podman
is daemonless, and Swarm mode is fundamentally a daemon-and-raft-cluster
architecture. Rootless Podman will not get Swarm, and that is a deliberate
architectural decision, not a missing feature.

The consequence: a "port to Podman" is not a client-library swap. Roughly
two-thirds of swarmctl's surface is Swarm-specific — `swarm init/join/leave`,
`cordon/drain/promote/demote`, services as a scheduler primitive, overlay
networks, the raft-based manager quorum. None of that has a Podman counterpart.

---

## 1. What survives a Podman port unchanged

This is the good news, and it is most of the codebase's *structure*.

Everything below is already engine-agnostic by design, because the architecture
separates `api/` (Bollard types) from `models/` (presentation) from `cli/` and
`tui/` (the only things that print).

- The layered architecture itself. `api/` is the only layer that knows about
  Bollard; swapping the client does not touch `models/`, `cli/`, or `tui/`.
- `specdiff.rs` — takes two `serde_json::Value`s, knows nothing about Docker.
  Entirely portable.
- `applied.rs` — filesystem paths and JSON. Entirely portable.
- `drift.rs` — depends only on `specdiff` and `applied`. Portable as-is.
- The selector engine, duration parser, CIDR expander, table/JSON/YAML printer.
  All pure logic.
- The TUI.
- The vault (`vault/`) — Argon2id + AES-256-GCM over local files.
- The endpoint registry (`endpoints.rs`) — mostly; see §4 for the transport
  assumptions.

Rough estimate: the pure-logic half of the codebase ports unchanged. That is
not a small thing — it is most of what makes swarmctl worth having.

---

## 2. What does not survive

Everything Swarm-shaped. In order of how central it is:

| Area | Why it dies |
|---|---|
| Services as a primitive | No scheduler. Nothing reschedules a failed task. |
| `swarm init/join/leave/token` | No raft cluster to join. |
| `cordon/uncordon/drain/taint` | No scheduler to influence. |
| `promote/demote` | No manager quorum. |
| Overlay networks, service mesh | Podman's built-in multi-host networking is limited compared to Swarm's. |
| `tasks` | No scheduler emitting task state. |
| `rollout status/history/undo` | No rollout state machine. |
| `scale` | Meaningless without a desired-state reconciler. |
| `diff-clusters` | Degenerates to comparing two single hosts. |
| `failover` | The policy presumes a scheduler that can be told where to place work. |

That is roughly the top two-thirds of the command surface listed in the README.

---

## 3. Option A — port to Podman's container API (drops orchestration)

Keep the CLI shape, point it at a different runtime, accept that orchestration
is gone.

**The client is the easy part.** Bollard already treats Podman as a first-class
runtime: `Docker::connect_with_podman_defaults()` probes `$DOCKER_HOST`, then
rootless Podman sockets, then the system Podman socket, and falls back to
Docker. Much of `api/` may work against Podman with only transport changes.
Alternatives if that proves insufficient:

- `podman_rest_client` — generated from Podman's official swagger, v5-focused,
  supports Unix socket and SSH. The crate's own docs warn v4 support is "in
  pretty terrible shape".
- `podtender` — async hyper/tokio client, tracks latest Podman only.
- `podman-api`, `podman-client` — smaller, less mature.

**The honest scope.** This yields a single-host and small-fleet container
manager: `get`, `describe`, `logs`, `exec`, `cp`, `top`, `events`, secrets,
configs, networks, volumes, `drift`. That is a genuinely useful tool and it is
not nothing. But it is a different product, and calling it swarmctl would be
misleading.

**Orchestration replacement.** The standard answer is Quadlet + systemd:
containers declared as native systemd units, with real dependency ordering,
restart policies, and journald integration. This is the correct way to run
Podman and it is what the ecosystem recommends. But it means the declarative
source of truth becomes `.container` unit files, not Compose — so `apply` would
need to write systemd units and `drift` would compare against `systemctl`
state. Note the upside: drift detection maps onto Quadlet *better* than it does
onto Swarm, because systemd units are the declared state and
`systemctl show` is queryable.

**Effort:** medium. Days to weeks. Most of it is deleting commands and
retargeting transports, not writing new logic.

---

## 4. Option B — port to Kubernetes (keeps the spirit)

This is the answer that actually preserves what swarmctl *is*: a
kubectl-inspired control plane for clusters too small for real Kubernetes.

The mapping is unusually clean because swarmctl was deliberately designed
kubectl-shaped:

| swarmctl | Kubernetes |
|---|---|
| `get`/`describe` | identical, including `-o json/yaml` and field selectors |
| `apply`/`replace`/`patch`/`diff` | server-side apply, the real thing |
| `rollout status/history/undo` | native, and better implemented |
| `cordon`/`drain` | native |
| `scale` | native |
| label and field selectors | native, and more expressive |
| **`drift`** | **`kubectl diff` + drift detection — swarmctl's new feature stays ahead here** |
| **last-applied store** | `kubectl.kubernetes.io/last-applied-configuration` annotation |

The irony worth stating plainly: swarmctl's two genuinely novel contributions
— structured drift detection and the last-applied manifest store — both map
onto Kubernetes *more naturally* than they do onto Swarm, because Kubernetes
has versioned objects and admission control where Swarm has neither. The
original design even grepped for `kubectl.kubernetes.io/last-applied`
before concluding no such annotation existed in this codebase.

**Client:** `kube-rs` (the maintained Rust client) or `k8s-openapi` for types.
This is a real rewrite of `api/` — Kubernetes' API is far larger and more
consistent, but it is a different API.

**Orchestration target for a homelab:** k3s, MicroK8s, or RKE2. Compose files
convert via `kompose` as a starting point, though output usually needs editing.

**Effort:** high. Weeks to months. But it produces a tool that stays relevant.

---

## 5. Option C — dual-runtime behind a trait (hedge)

Define a `Runtime` trait covering the portable operations, implement it for
Bollard-Docker today, and add a Podman or Kubernetes implementation later.

```rust
#[async_trait]
pub trait Runtime {
    async fn list_services(&self) -> Result<Vec<Value>>;
    async fn inspect(&self, kind: &str, name: &str) -> Result<Value>;
    async fn logs(&self, name: &str, opts: &LogOptions) -> Result<()>;
    // ... the portable surface only
}
```

`drift`, `specdiff`, `applied`, and every pure-logic module sit above this
trait unchanged. Swarm-only commands are gated behind a capability check that
returns a clear "not supported on this runtime" rather than silently doing
nothing — which is the codebase's existing stated convention.

**Effort:** medium-high *now*, but it front-loads the cost and means the
runtime decision can be deferred. The risk is designing a trait against one
implementation and getting the abstraction wrong.

---

## 6. Recommendation

Do nothing yet, and do not port.

The premise is weaker than it looks. Swarm mode is maintained, commercially
supported by Mirantis, and had Swarm-specific fixes in the September 2026
Engine release. Nothing has an end-of-life date. A port now spends effort
defending against a risk that has not materialised.

Two cheap things worth doing regardless, because they are useful today:

1. **Keep the layering honest.** The `api/` → `models/` → `cli/` separation is
   already the right abstraction and already makes most of the code portable.
   Resist changes that leak Bollard types upward into `cli/`. That discipline
   is what makes any future port a bounded job rather than a rewrite.

2. **Finish drift for the other resource kinds.** Configs, secrets, and
   networks. This is worth doing on Swarm today, and it is exactly the work
   that survives every option above — Option A, B, and C all keep it.

If the trigger ever fires — a real EOL announcement, or SwarmKit going quiet
for a year — the decision tree is:

- Still want a *cluster* tool → **Option B**, Kubernetes. It is the only option
  that preserves the product's reason to exist, and drift ports into it
  unusually well.
- Only ever ran one or two hosts → **Option A**, Podman + Quadlet, and accept
  the product becomes a single-host manager.
- Genuinely unsure → **Option C**, but only after the runtime is actually
  needed, so the trait is designed against two real implementations rather
  than one imagined one.

The one thing to avoid is porting to Podman *because it is the Docker CLI
replacement*. Podman replaces the daemon. swarmctl is not a daemon wrapper — it
is an orchestrator wrapper. Those need different successors.
