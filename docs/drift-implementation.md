# Drift detection: implementation record

This document covers the drift-detection feature built across this session:
what was added, what changed, every test written, the three real bugs found
against the live cluster, and the known gaps.

Verified state at time of writing: 340 tests passing, `clippy -D warnings`
clean, `fmt --check` clean, release and `--no-default-features` builds both
succeeding, and the full chain exercised against a live 5-node Swarm cluster.

---

## 1. The problem

Docker Swarm has no admission controllers and no versioned objects. A service
edited by hand, or an image tag silently repointed by a push, is invisible:
`docker service inspect` and `swarmctl get services` both report only what is
running *now*. Nothing in the ecosystem records what a cluster was *asked* to
run, so there is no way to tell drift apart from the declared state.

`drift` is the missing half: `apply` records the manifest it submits, `drift`
compares the live cluster against those records.

---

## 2. New files

### `src/api/specdiff.rs` (308 lines, 14 tests)

Structured, field-aware comparison of a live object against a manifest.

Replaces the previous implementation, which shelled out to the system
`diff -u` on fully serialized Bollard objects. Live objects carry
server-assigned noise (`Version`, `CreatedAt`, `UpdateStatus`, node
bookkeeping) that no manifest can reproduce, so every run printed a wall of
false positives that buried the real change.

Key exports:

- `IGNORED_FIELDS` — server-assigned fields, matched by last path segment so
  the same rule applies at any nesting depth. Includes `Version`, `CreatedAt`,
  `Created`, `UpdatedAt`, `UpdateStatus`, `ServiceStatus`, `VirtualIPs`,
  `AssignedGenericResources`, `DisruptiveUpdates`, `JobStatus`, and
  `ForceUpdate`.
- `compare_objects(live, desired)` — recursive leaf comparison, returns
  `Vec<FieldChange>` sorted by dotted path.
- `compare_sets(live, desired)` — name-keyed resource sets, reports changed /
  added / removed.
- `render_value` — strings print bare; everything else as compact JSON;
  absent as `<absent>`.

Container length is deliberately ignored, because it is derived from the
children.

### `src/api/applied.rs` (417 lines, 13 tests)

Last-applied manifest store, keyed per endpoint.

Layout: `~/.swarmctl/applied/<endpoint>/<kind>/<name>.json`. The endpoint
segment keeps several clusters from sharing one namespace. Files are written
`0600` via the existing `write_private` helper.

Every accessor has a `_at` variant taking an explicit base directory. That is
how the tests point the store at a temp dir — the production paths all resolve
through `swarmctl_dir()`, which is process-wide, so mutating it from a test
would race other tests running in parallel. (An earlier version did exactly
that via a `SWARMCTL_DIR` env var and produced five intermittently-failing
tests; the explicit-base threading is the fix.)

Exports: `record`/`record_at`, `read`/`read_at`, `list`/`list_at`,
`remove`/`remove_at`, `endpoints_with_state`/`_at`, `record_path`/`_at`,
`base_dir_at`, `is_safe_name`, `AppliedRecord`.

`is_safe_name` rejects empty, `.`, `..`, anything containing `/`, `\`, or NUL,
so a manifest named `../../vault` cannot escape the store.

### `src/cli/drift.rs` (491 lines, 18 tests)

The `drift` and `drift-forget` commands.

`drift` compares live services against recorded manifests and exits 0 when
everything matches, 1 when anything drifted — so `drift && deploy.sh` gates a
pipeline on declared state being true. Supports `-o table|json|yaml`.

Reports three distinct things:

- **Drifted** — the field no longer matches the manifest. Affects exit code.
- **Missing** — recorded but gone from the cluster. Affects exit code.
- **Untracked** — running with no recorded manifest. Does *not* affect exit
  code, because it is a fact about the store rather than a difference.

`drift-forget` clears recorded manifests: no arguments clears the current
endpoint, an endpoint name clears one, `--all` clears everything. It runs
before any Docker connection is built, so it works with no daemon reachable.

---

## 3. Changed files

| File | Change |
|---|---|
| `src/api/mod.rs` | Register `applied` and `specdiff` modules. |
| `src/cli/mod.rs` | Register `drift` module. |
| `src/cli/apply.rs` | Record manifests after a successful apply; real field-level diff on `--dry-run`; `ApplyOptions` struct. |
| `src/cli/delete.rs` | Clear the recorded manifest on delete; `host` in `DeleteOptions`. |
| `src/cli/diff.rs` | Replace the `diff -u` shell-out with structured comparison. |
| `src/cli/fanout.rs` | Drift fan-out across endpoints. |
| `src/cli/root.rs` | `Drift` and `DriftForget` commands; global flag fixes. |
| `src/utils/paths.rs` | `SWARMCTL_DIR` override on `swarmctl_dir()`. |
| `README.md` | Drift section, command table, architecture, roadmap. |

### `apply` — records and real dry-run

After a successful apply, the manifest is written to the store. A failure to
record is logged as a warning but does not fail the apply: the cluster is
already in the requested state, and losing drift tracking is strictly less bad
than rolling that back.

`--dry-run` previously printed "would apply service X" without comparing
anything. It now runs the real structured diff and reports either "already
matches; no changes would be applied" or the field-level changes. A dry run
that shows no differences is the difference between "this apply is a no-op" and
"this apply would change the cluster".

`apply::run` takes an `ApplyOptions` struct rather than eight positional
arguments, two of which were same-typed `Option`s and so silently swappable at
the call site — which is exactly how the host argument got dropped in the first
place.

### `delete` — clears the record

Without this, a deleted resource stays in the drift store forever and `drift`
reports it as "recorded but absent from the cluster" on every subsequent run.

### `diff` — no longer shells out

Same ignore list and same rendering as drift, so `diff` and `drift` cannot
disagree about what counts as a difference. Exit code convention preserved:
0 when identical, 1 when they differ.

### `fanout` — drift across endpoints

`drift --endpoints prod-eu,prod-us` runs against each and prints a per-cluster
report. One unreachable cluster does not hide the others' results. Exits 1 if
any endpoint drifted.

Collected directly rather than through `for_each_endpoint`, whose
`EndpointOutcome` type discards the payload — and the drift report *is* the
payload here.

---

## 4. Every test written

### `src/api/specdiff.rs` (14)

| Test | Pins |
|---|---|
| `identical_objects_report_no_changes` | Baseline: no false positives. |
| `reports_a_differing_scalar_with_dotted_path` | Path is dotted, values rendered. |
| `server_assigned_version_fields_are_ignored` | The three fields that made the old `diff -u` useless. |
| `ignored_fields_are_skipped_at_any_depth` | Ignore list matches on last segment, not full path. |
| `a_field_missing_from_the_desired_side_is_reported` | Absent renders as `<absent>`, not silently dropped. |
| `container_length_does_not_matter` | Array length alone is never a diff. |
| `empty_arrays_and_objects_compare_equal` | Empty vs empty is equal. |
| `numeric_difference_is_reported_as_strings` | Numerics compared by value, rendered as strings. |
| `changes_are_sorted_by_path` | Deterministic output order. |
| `compare_sets_reports_changed_resources` | Changed resources carry field diffs. |
| `compare_sets_reports_missing_and_extra_resources` | Added and removed both surface. |
| `compare_sets_of_identical_sets_is_empty` | No-op case. |
| `render_value_prints_strings_barely` | `nginx` not `"nginx"`. |
| `is_ignored_matches_the_constant_list` | The ignore list is the single source of truth. |

### `src/api/applied.rs` (13)

| Test | Pins |
|---|---|
| `is_safe_name_rejects_path_traversal` | `..`, `/`, `\`, NUL all rejected. |
| `record_then_read_round_trips` | Write then read returns the same manifest. |
| `read_of_an_unrecorded_resource_is_none` | Absent is `Ok(None)`, not an error. |
| `record_refuses_an_unsafe_name` | Traversal is refused at write time. |
| `list_returns_every_recorded_resource` | All resources in a kind are listed. |
| `list_of_an_unknown_endpoint_is_empty_not_an_error` | Unknown endpoint is empty. |
| `records_are_scoped_to_one_endpoint` | Two endpoints never see each other's records. |
| `endpoints_with_state_lists_the_populated_endpoints` | Enumeration for `--all`. |
| `remove_drops_only_that_record` | Delete is surgical. |
| `remove_of_an_absent_record_is_not_an_error` | Idempotent. |
| `record_overwrites_the_previous_manifest` | Re-apply replaces, never appends. |
| `endpoints_with_state_is_empty_without_the_directory` | No dir, no error. |
| `a_recorded_manifest_is_owner_only` (unix) | Mode is `0600` — a manifest can reference secrets. |

### `src/cli/drift.rs` (18)

| Test | Pins |
|---|---|
| `a_matching_cluster_reports_no_drift` | Clean case, exit 0. |
| `a_moved_image_tag_is_reported_as_drift` | The core case: a tag repointed by a push. |
| `a_hand_edited_replica_count_is_reported` | Manual edits surface. |
| `a_recorded_resource_gone_from_the_cluster_is_missing` | Missing, not drifted. |
| `a_resource_running_without_a_manifest_is_untracked` | Untracked does not affect exit code. |
| `server_assigned_version_fields_never_show_up_as_drift` | `ForceUpdate` ignored. |
| `several_resources_are_reported_independently` | Per-resource, not aggregate. |
| `render_report_names_the_resource_and_the_field` | Output format. |
| `render_report_says_so_when_there_is_no_drift` | Clean output says so. |
| `render_report_lists_missing_and_untracked` | Both listed distinctly. |
| `json_output_carries_the_drift_details` | Machine-readable shape. |
| `json_output_marks_a_clean_cluster` | `clean: true`. |
| `services_are_driftable_and_tasks_are_not` | Scope guard. |
| `forget_targets_removes_every_recorded_manifest` | Forget clears the store. |
| `forget_targets_of_an_unknown_endpoint_removes_nothing` | No-op, no error. |
| `forget_targets_of_no_endpoints_is_zero` | Empty input. |
| `report_forgotten_distinguishes_empty_from_populated` | Both branches. |
| `store_kinds_cover_the_resource_types_apply_writes` | Services covered. |

### `src/cli/apply.rs` (4, new)

| Test | Pins |
|---|---|
| `spec_name_accepts_the_api_capitalised_spelling` | `Name` from `get -o json`. |
| `spec_name_accepts_a_lowercase_manifest` | `name` from a hand-written file. |
| `spec_name_prefers_the_api_spelling_when_both_exist` | Precedence. |
| `spec_name_is_none_when_absent_or_empty` | `None`, not `""`. |

### `src/cli/root.rs` (2, new)

| Test | Pins |
|---|---|
| `global_host_and_context_flags_are_accepted` | `--host`/`--context` parse in both positions, including before a subcommand with positionals. |
| `a_global_host_does_not_consume_a_positional_name` | The exact bug: `--host` must not become the `NAME` positional. |

### `src/cli/delete.rs` (2, new)

| Test | Pins |
|---|---|
| `delete_options_carries_the_host_separately_from_the_name` | Fields stay distinct. |
| `a_name_is_never_confused_with_a_host` | Guard against the swapped-parameter class of bug. |

### `src/api/endpoints.rs` (2, new)

| Test | Pins |
|---|---|
| `a_cli_host_override_matches_the_registered_endpoint` | An explicit `--host` never keys as `local`. |
| `no_host_and_no_env_falls_back_to_local` | The genuine fallback. |

Plus `current_endpoint_key_falls_back_without_a_registry`, which lost its
`#[test]` attribute during an edit and was restored.

**Total: 45 new tests, 340 passing.**

---

## 5. The three real bugs the live cluster found

All three were invisible to the unit tests and only surfaced against a real
5-node Swarm.

### Bug 1 — `Name` vs `name`

`apply` read the resource name from lowercase `name`, but `get services -o
json` emits the spec with `Name` capitalised, as the Docker API returns it.
Every applied service was recorded under the literal string `"unknown"`, so
`drift` reported it as both missing *and* untracked at once.

Fix: `spec_name()` accepts both spellings, preferring the API form. Four tests.

### Bug 2 — the manifest store keyed to the wrong cluster

`current_endpoint_key()` only consulted `DOCKER_HOST`, and the CLI's `--host`
flag never writes to that variable. So `swarmctl --host ssh://zev apply -f
web.yml` recorded under `local`, and `drift` against the same `--host` then
found no records at all.

Fix: `current_endpoint_key_with(host_override)`, threaded through apply,
delete, drift, and drift-forget. Two tests.

### Bug 3 — swapped parameters in `delete_service`

The signature was `(opts, host, name, selector)` while the call site passed
`(opts, name, selector, host)`. Both sides were `Option<String>`, so the
compiler could not catch the swap. `swarmctl --host ssh://x delete services
web` therefore tried to delete a service literally named `ssh://x` and
silently ignored the real name.

This was introduced during the host-threading refactor and initially
misdiagnosed as a clap argument-binding problem. Instrumenting the real binary
with debug prints showed clap binding `name=Some("zev_drift_test")` correctly
all the way into `delete::run`, which located it.

Fix: reorder the signature to match the call site. Two tests, plus
`--host`/`--context`/TLS flags made `global = true`.

**`-c` is deliberately not global**: `stack deploy -c/--compose-file` already
owns that short, and clap requires global shorts to be unique across every
subcommand. `--context` parses in either position; `-c` only before the
subcommand. That collision surfaced as a clap debug-assert test failure, which
is how it was caught.

---

## 6. Live verification against zev

Registered `zev` as `ssh://kyle@192.168.7.7` (manager, 5 nodes, Docker 29.9.0,
swarm "default").

1. `apply services -f spec.json` → recorded at
   `~/.swarmctl/applied/zev/services/zev_drift_test.json`
2. `drift` → `zev: no drift in tracked resources`, exit 0
3. Changed the memory limit on zev behind swarmctl's back via `docker service
   update --limit-memory`
4. `drift` → exit 1, with exactly:

       zev: 1 resource(s) drifted
       ~ service zev_drift_test
             TaskTemplate.Resources.Limits.MemoryBytes: 268435456 -> 536870912

   Server-noise fields correctly absent.
5. `drift -o json` → valid machine-readable output
6. `drift-forget` → `Forgot 1 recorded manifest(s) across 1 endpoint(s).`
7. `delete services zev_drift_test --host ssh://…` → deleted, drift record
   cleared
8. Test service removed; zev back to its original 23 services

One test attempt was invalid rather than the code being wrong: an `apply` with
an edited spec re-recorded the manifest *before* the SSH update, so the two
changes matched. The valid sequence changes the cluster without going through
`apply`.

---

## 7. Known gaps

- **Drift covers services only.** Configs, secrets, and networks can be stored
  but are not compared; a live→manifest mapping for each needs a decision
  about which fields are declarative.
- **Untested against other clusters.** Verified against one live Swarm.
- **`ForceUpdate` is ignored by necessity.** `rollout restart` increments it
  deliberately, so a restart shows as no drift. A restart is not a spec change,
  which is the correct behaviour, but it does mean a restart is invisible.
- **No `--watch` on drift.** It is a point-in-time check.
- **The store is never pruned automatically.** `drift-forget` is manual.
