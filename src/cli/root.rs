use crate::api::client::DockerClient;
use crate::cli::{
    api_resources, apply, attach, cluster, completion, config, context, cordon, cp, create, delete,
    describe, diff, discover, drift, edit, endpoint, events, exec, explain, failover, fanout, get,
    label, logs, patch, port_forward, ports, replace, rollout, run, scale, set, stack, swarm,
    system, taint, top, vault, volume, wait,
};
#[cfg(feature = "tui")]
use crate::tui;
use clap::{Parser, Subcommand};
use clap_complete::Shell;

#[derive(Parser, Clone)]
#[command(name = "swarmctl")]
#[command(about = "Docker Swarm control plane CLI")]
#[command(long_about = "A kubectl-inspired CLI for managing Docker Swarm clusters")]
pub struct Cli {
    /// Output format
    #[arg(short, long, value_parser = parse_output_format, default_value = "table")]
    pub(crate) output: OutputFormat,

    /// Watch for changes
    // Global so it works in either position: `swarmctl --watch get services`
    // and `swarmctl get services --watch`.
    #[arg(short, long, global = true)]
    watch: bool,

    /// Docker daemon host (e.g. tcp://192.168.1.100:2376, ssh://user@host, unix:///var/run/docker.sock)
    ///
    /// Global so it parses in either position, like `--watch`. Without
    /// `global = true` clap rejects `--host` after a subcommand outright, and
    /// before a subcommand with a positional NAME it binds the host value to
    /// that positional instead — so `swarmctl --host ssh://x delete services
    /// web` tried to delete a service literally named `ssh://x`.
    #[arg(long, global = true, env = "DOCKER_HOST")]
    host: Option<String>,

    /// Docker context to use (overrides DOCKER_HOST and current context)
    ///
    /// Global, but only under its long name: `-c` is already taken by
    /// `stack deploy -c/--compose-file`, and a global short must be unique
    /// across every subcommand. `--context` still parses in either position;
    /// `-c` only works before the subcommand.
    #[arg(long, global = true, env = "DOCKER_CONTEXT")]
    context: Option<String>,

    /// Path to TLS CA certificate
    ///
    /// Global for the same reason as `--host`: these must survive being
    /// written either side of the subcommand.
    #[arg(long, global = true)]
    tlscacert: Option<String>,

    /// Path to TLS client certificate
    #[arg(long, global = true)]
    tlscert: Option<String>,

    /// Path to TLS client key
    #[arg(long, global = true)]
    tlskey: Option<String>,

    /// Comma-separated endpoint names to run this command against
    ///
    /// Registered with `swarmctl endpoint add`. Runs the command against every
    /// listed endpoint and combines the results.
    #[arg(long, global = true, value_name = "NAMES")]
    pub(crate) endpoints: Option<String>,

    /// Assume yes to confirmation prompts (required for write fan-out in scripts)
    #[arg(short = 'y', long, global = true)]
    pub(crate) yes: bool,

    /// Verbose output level (repeatable: -v info, -vv debug, -vvv trace)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    pub(crate) command: Commands,
}

/// Reject flag combinations that used to be silently resolved in one direction.
///
/// `--context` previously won outright and every `--host`/`--tls*` flag was
/// discarded, so `swarmctl --host tcp://x --context prod get nodes` quietly
/// connected somewhere other than where the command line said. Docker treats
/// this as a conflict; so does swarmctl now.
pub fn connection_conflict(
    context: Option<&str>,
    host: Option<&str>,
    // TLS flags alongside --context are a warning rather than an error (the
    // context carries its own), so this is part of the signature for symmetry
    // and for callers that want to know, not for the conflict rule itself.
    _tls_requested: bool,
) -> anyhow::Result<()> {
    if let (Some(ctx), Some(_)) = (context, host) {
        anyhow::bail!(
            "cannot combine --context {ctx} with --host: a context already \
             names a daemon. Drop one, or use --host alone."
        );
    }
    Ok(())
}

/// Clap entry point for `-o`. Surfaces [`OutputFormat::parse`]'s message so a
/// bad `-o` value explains itself instead of printing clap's enum list.
fn parse_output_format(value: &str) -> Result<OutputFormat, String> {
    OutputFormat::parse(value).map_err(|e| e.to_string())
}

/// `-o` output format.
///
/// Unlike the previous fixed `ValueEnum`, this carries a payload so kubectl's
/// compound forms work: `-o jsonpath={.metadata.name}`, `-o custom-columns=A:.x`,
/// `-o go-template={{...}}`. Parsed with [`OutputFormat::parse`], which is wired
/// into clap via `value_parser`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OutputFormat {
    Table,
    Wide,
    Json,
    Yaml,
    Name,
    /// `-o jsonpath=<template>`
    JsonPath(String),
    /// `-o jsonpath-as-json=<template>`
    JsonPathAsJson(String),
    /// `-o go-template=<template>`
    GoTemplate(String),
    /// `-o custom-columns=<NAME>:<path>,...`
    CustomColumns(String),
}

impl OutputFormat {
    /// Parse the `-o` value, kubectl-style.
    ///
    /// Accepts a bare format name or `name=payload`. Splitting happens at the
    /// *first* `=` so a jsonpath template containing `==` survives intact.
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        let v = value.trim();
        let (name, payload) = match v.split_once('=') {
            Some((n, p)) => (n.trim(), Some(p)),
            None => (v, None),
        };
        let needs_payload = |p: Option<&str>| -> anyhow::Result<String> {
            p.filter(|s| !s.is_empty())
                .map(str::to_string)
                .ok_or_else(|| {
                    anyhow::anyhow!("-o {name} requires an argument, e.g. -o {name}=<expression>")
                })
        };
        Ok(match name {
            "table" => OutputFormat::Table,
            "wide" => OutputFormat::Wide,
            "json" => OutputFormat::Json,
            "yaml" => OutputFormat::Yaml,
            "name" => OutputFormat::Name,
            "jsonpath" => OutputFormat::JsonPath(needs_payload(payload)?),
            "jsonpath-as-json" => OutputFormat::JsonPathAsJson(needs_payload(payload)?),
            "go-template" => OutputFormat::GoTemplate(needs_payload(payload)?),
            "custom-columns" => OutputFormat::CustomColumns(needs_payload(payload)?),
            other => anyhow::bail!(
                "unable to match a printer suitable for the output format \"{other}\"; \
                 supported: table, wide, json, yaml, name, \
                 jsonpath=EXPR, jsonpath-as-json=EXPR, go-template=EXPR, custom-columns=SPEC"
            ),
        })
    }

    /// True for formats that are meant to be piped, not read.
    ///
    /// The connection banner is suppressed for these so `-o json | jq` is clean.
    pub fn is_machine(&self) -> bool {
        !matches!(
            self,
            OutputFormat::Table | OutputFormat::Wide | OutputFormat::Name
        )
    }

    /// The `-o` spelling, for round-tripping in error messages.
    pub fn as_arg(&self) -> String {
        match self {
            OutputFormat::Table => "table".into(),
            OutputFormat::Wide => "wide".into(),
            OutputFormat::Json => "json".into(),
            OutputFormat::Yaml => "yaml".into(),
            OutputFormat::Name => "name".into(),
            OutputFormat::JsonPath(t) => format!("jsonpath={t}"),
            OutputFormat::JsonPathAsJson(t) => format!("jsonpath-as-json={t}"),
            OutputFormat::GoTemplate(t) => format!("go-template={t}"),
            OutputFormat::CustomColumns(t) => format!("custom-columns={t}"),
        }
    }
}

#[derive(Clone, Subcommand)]
pub(crate) enum Commands {
    /// Display one or many resources
    Get {
        /// Resource type. Accepts plural, singular and short names, may be
        /// comma-separated, may carry a `.VERSION.GROUP` qualifier, and may name
        /// objects inline with `TYPE/NAME`.
        ///
        /// Examples: `svc`, `po,svc`, `po/web`, `services.v1.`
        resource: String,

        /// Resource name(s). May be repeated.
        #[arg(num_args = 0..)]
        name: Vec<String>,

        /// Show labels
        #[arg(long)]
        show_labels: bool,

        /// Include the resource type when printing names
        #[arg(long)]
        show_kind: bool,

        /// Do not print the table header
        #[arg(long)]
        no_headers: bool,

        /// Extra label column(s) to display; repeatable or comma-separated
        #[arg(short = 'L', long, value_name = "LABEL", value_delimiter = ',')]
        label_columns: Vec<String>,

        /// Filter by label selector: key=value, key==v, key!=v, key, !key,
        /// key in (a,b), key notin (a,b). Comma-separated terms are ANDed.
        #[arg(short = 'l', long)]
        selector: Option<String>,

        /// Filter by field selector against the Kubernetes envelope
        /// (e.g. metadata.name=web, status.state=ready)
        #[arg(long)]
        field_selector: Option<String>,

        /// Sort by a jsonpath expression (e.g. '{.metadata.name}')
        #[arg(long)]
        sort_by: Option<String>,

        /// Output format (table, json, yaml, wide, name)
        #[arg(short, long, value_parser = parse_output_format)]
        output: Option<OutputFormat>,
    },

    /// Show detailed information about a resource
    Describe {
        /// Resource type (plural, singular or short name)
        resource: String,

        /// Resource name (optional if using --selector)
        name: Option<String>,

        /// Filter by label selector
        #[arg(long)]
        selector: Option<String>,
    },

    /// Create a resource from a file or stdin
    Create {
        /// Resource type (plural, singular or short name)
        resource: String,

        /// Resource name
        name: Option<String>,

        /// Filename or directory to read from
        #[arg(short, long)]
        filename: Option<String>,

        /// Create from stdin
        #[arg(short = 'i')]
        stdin: bool,

        /// Dry run mode (validate only)
        #[arg(long)]
        dry_run: bool,

        /// Output format for the resulting object
        #[arg(short, long, value_parser = parse_output_format)]
        output: Option<OutputFormat>,
    },

    /// Delete resources
    Delete {
        /// Resource type (plural, singular or short name)
        resource: String,

        /// Resource name
        name: Option<String>,

        /// Selector to filter resources
        #[arg(long)]
        selector: Option<String>,

        /// Dry run mode (validate only)
        #[arg(long)]
        dry_run: bool,

        /// Ignore not found errors
        #[arg(long)]
        ignore_not_found: bool,

        /// Grace period in seconds (default 30)
        #[arg(long)]
        grace_period: Option<i64>,

        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,

        /// Wait for deletion to complete
        #[arg(long)]
        wait: bool,
    },

    /// Scale a service
    Scale {
        /// Service name
        name: String,

        /// Number of replicas
        replicas: u64,
    },

    /// Fetch the logs of a resource
    Logs {
        /// Resource type (service or task)
        resource: String,

        /// Resource name
        name: String,

        /// Follow log output
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to show from the end
        #[arg(short, long, default_value = "100")]
        tail: i64,

        /// Show logs from previous (terminated) instances
        #[arg(long)]
        previous: bool,

        /// Show timestamps on each line
        #[arg(long)]
        timestamps: bool,

        /// Only return logs newer than a relative duration (e.g. 5s, 2m, 3h)
        #[arg(long)]
        since: Option<String>,

        /// Add a prefix to each log line with the pod name
        #[arg(long)]
        prefix: bool,

        /// Ignore log stream errors
        #[arg(long)]
        ignore_errors: bool,
    },

    /// List and visualize port mappings on the swarm
    Ports {
        /// Enable TUI visualization mode
        #[arg(short, long)]
        #[cfg(feature = "tui")]
        tui: bool,

        /// Show available ports in the specified range
        #[arg(short, long)]
        available: bool,

        /// Start of port range to scan
        #[arg(long, default_value = "30000")]
        range_start: Option<u16>,

        /// End of port range to scan
        #[arg(long, default_value = "40000")]
        range_end: Option<u16>,

        /// Filter by protocol (tcp or udp)
        #[arg(short, long)]
        protocol: Option<String>,
    },

    /// Get cluster info
    ClusterInfo,

    /// Launch interactive TUI dashboard
    #[cfg(feature = "tui")]
    Dashboard,

    /// Manage Docker contexts
    Context {
        #[command(subcommand)]
        command: ContextCommand,
    },

    /// Stack operations
    Stack {
        #[command(subcommand)]
        command: StackCommand,
    },

    /// Create or update a resource from a file or stdin
    Apply {
        /// Resource type (plural, singular or short name)
        resource: String,

        /// Resource name
        name: Option<String>,

        /// Filename to read from
        #[arg(short, long)]
        filename: Option<String>,

        /// Create from stdin
        #[arg(short = 'i')]
        stdin: bool,

        /// Dry run mode (validate only)
        #[arg(long)]
        dry_run: bool,

        /// Output format for the resulting object
        #[arg(short, long, value_parser = parse_output_format)]
        output: Option<OutputFormat>,
    },

    /// Run an ad-hoc service
    Run {
        /// Service name
        name: String,

        /// Container image
        #[arg(short, long)]
        image: String,

        /// Number of replicas
        #[arg(short, long, default_value = "1")]
        replicas: u64,

        /// Environment variables (KEY=VAL)
        #[arg(short, long)]
        env: Vec<String>,

        /// Labels (KEY=VAL)
        #[arg(short, long)]
        labels: Vec<String>,

        /// Network to attach
        #[arg(short, long)]
        network: Option<String>,

        /// Publish a port: `container`, `host:container`, or either with `/proto`
        #[arg(short, long)]
        publish: Vec<String>,

        /// Output format for the created service object
        #[arg(short, long, value_parser = parse_output_format)]
        output: Option<OutputFormat>,
    },

    /// Stream Docker swarm events
    Events {
        /// Filter by event type (container, image, service, node, network, etc.)
        #[arg(long = "filter-type", value_name = "TYPE")]
        filter_types: Vec<String>,

        /// Show events created since this timestamp (e.g. "1h", "30m", or unix timestamp)
        #[arg(long)]
        since: Option<String>,
    },

    /// Execute a command in a running container
    Exec {
        /// Service name
        service: String,

        /// Command to execute
        #[arg(trailing_var_arg = true)]
        command: Vec<String>,
    },

    /// Attach to a running container's stdio
    Attach {
        /// Service name
        service: String,

        /// Interactive mode (stdin)
        #[arg(short, long)]
        interactive: bool,
    },

    /// Show resource utilization
    Top {
        #[command(subcommand)]
        command: TopCommand,
    },

    /// Forward local port to a container port in a service
    PortForward {
        /// Service name
        service: String,

        /// Local port
        local_port: u16,

        /// Container port
        container_port: u16,
    },

    /// Copy files between containers and the local filesystem
    Cp {
        /// Source path (service:path or local-path)
        source: String,

        /// Target path (local-path or service:path)
        target: String,
    },

    /// Manage rollouts of a service
    Rollout {
        #[command(subcommand)]
        command: RolloutCommand,
    },

    /// Show version information
    Version,

    /// Update labels on a resource
    Label {
        /// Resource type (node, service, secret, config)
        resource: String,

        /// Resource name
        name: Option<String>,

        /// Labels to set (KEY=VAL)
        labels: Vec<String>,

        /// Overwrite existing labels (set only these)
        #[arg(long)]
        overwrite: bool,

        /// Delete all labels on the resource
        #[arg(long)]
        all: bool,

        /// Selector to filter resources
        #[arg(long)]
        selector: Option<String>,
    },

    /// Set specific fields on resources
    Set {
        #[command(subcommand)]
        command: SetCommand,
    },

    /// Show documentation for a resource type
    Explain {
        /// Resource type (omit to list them all)
        resource: Option<String>,
    },

    /// Drain a node (set to drain availability)
    Drain {
        /// Node name
        name: String,
    },

    /// Edit a resource
    Edit {
        /// Resource type (service, node, secret, config)
        resource: String,

        /// Resource name
        name: String,
    },

    /// Diff a resource against a file
    Diff {
        /// Resource type (service, node, secret, config)
        resource: String,

        /// Resource name
        name: String,

        /// File to diff against
        #[arg(short, long)]
        filename: String,
    },

    /// Report services whose live state no longer matches the last apply
    Drift {
        /// Output format (table, json, yaml)
        #[arg(short, long, value_parser = parse_output_format)]
        output: Option<OutputFormat>,
    },

    /// Forget recorded manifests so `drift` stops tracking them
    ///
    /// With no arguments, clears every recorded manifest for the current
    /// endpoint. Pass an endpoint name to clear a retired cluster's records.
    DriftForget {
        /// Endpoint whose recorded manifests should be discarded
        endpoint: Option<String>,

        /// Discard the manifests of every endpoint with recorded state
        #[arg(long)]
        all: bool,
    },

    /// Apply a JSON merge patch to a resource
    Patch {
        /// Resource type (services, nodes, secrets, configs)
        resource: String,

        /// Resource name
        name: String,

        /// Patch content (inline or file with @prefix)
        #[arg(short, long)]
        patch: String,

        /// Patch type (only 'merge' is implemented; other values are rejected)
        #[arg(long, default_value = "merge")]
        patch_type: String,

        /// Dry run mode
        #[arg(long)]
        dry_run: bool,
    },

    /// Mark a node as unschedulable
    Cordon {
        /// Node name
        name: String,
    },

    /// Mark a node as schedulable
    Uncordon {
        /// Node name
        name: String,
    },

    /// Wait for a resource to reach a condition
    Wait {
        /// Resource type (service, task or node)
        resource: String,

        /// Resource name
        name: String,

        /// Condition to wait for
        condition: String,

        /// Timeout in seconds
        #[arg(short, long, default_value = "60")]
        timeout: u64,
    },

    /// Manage configs
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },

    /// Generate shell completion scripts
    Completion {
        /// Shell to generate completion for (bash, zsh, fish, powershell, elvish)
        shell: Shell,
    },

    /// List available API resources
    ApiResources,

    /// Add or update taints on a node
    Taint {
        /// Node name
        name: String,

        /// Taints to set (key=value:effect)
        taints: Vec<String>,

        /// Remove matching taints
        #[arg(short, long)]
        remove: Vec<String>,

        /// Overwrite existing taints
        #[arg(long)]
        overwrite: bool,
    },

    /// Replace a resource from a file or stdin
    Replace {
        /// Resource type (plural, singular or short name)
        resource: String,

        /// Filename to read from
        #[arg(short, long)]
        filename: Option<String>,

        /// Create from stdin
        #[arg(short = 'i')]
        stdin: bool,

        /// Dry run mode
        #[arg(long)]
        dry_run: bool,

        /// Replace an existing resource of the same name
        #[arg(long)]
        force: bool,

        /// Output format for the resulting object
        #[arg(short, long, value_parser = parse_output_format)]
        output: Option<OutputFormat>,
    },

    /// Manage swarm lifecycle
    Swarm {
        #[command(subcommand)]
        command: SwarmCommand,
    },

    /// Compare resources between two endpoints
    DiffClusters {
        /// Left endpoint name
        left: String,

        /// Right endpoint name
        right: String,

        /// Resource kind to compare (tasks are excluded: their ids are per-run)
        #[arg(long, default_value = "services")]
        resource: String,

        /// Output format
        #[arg(short, long, value_parser = parse_output_format, default_value = "table")]
        output: OutputFormat,
    },

    /// Manage the local vault
    Vault {
        #[command(subcommand)]
        command: VaultCommand,
    },

    /// Discover Docker hosts on the network and optionally join a swarm
    Discover {
        /// Subnet to scan in CIDR notation (e.g. 192.168.1.0/24). Auto-detected if omitted.
        #[arg(long)]
        subnet: Option<String>,

        /// Output results as JSON (scan only, no interactive join)
        #[arg(long)]
        json: bool,

        /// Skip the interactive TUI and use plain-text prompts
        #[arg(long)]
        no_tui: bool,
    },

    /// Promote a worker node to manager
    Promote {
        /// Node name or ID
        name: String,
    },

    /// Demote a manager node to worker
    Demote {
        /// Node name or ID
        name: String,
    },

    /// Manage Docker volumes
    Volume {
        #[command(subcommand)]
        command: VolumeCommand,
    },

    /// System-level commands (info, df, ping)
    System {
        #[command(subcommand)]
        command: SystemCommand,
    },

    /// Manage named remote endpoints (clusters)
    Endpoint {
        #[command(subcommand)]
        command: EndpointCommand,
    },

    /// Manage container failover and data migration
    Failover {
        #[command(subcommand)]
        command: FailoverCommand,
    },
}

#[derive(Clone, Subcommand)]
pub enum StackCommand {
    /// Deploy a stack from a compose file
    Deploy {
        /// Compose file path
        #[arg(short = 'c', long = "compose-file")]
        compose_file: String,

        /// Stack name
        name: String,
    },
    /// Remove a stack and its resources
    Rm {
        /// Stack name
        name: String,
    },
    /// List stacks
    Ls,
}

#[derive(Clone, Subcommand)]
pub enum ContextCommand {
    /// List available Docker contexts
    Ls,
    /// Switch to a Docker context
    Use {
        /// Context name
        name: String,
    },
    /// Inspect a Docker context
    Inspect {
        /// Context name
        name: String,

        /// Output format
        #[arg(short, long, value_parser = parse_output_format)]
        output: Option<OutputFormat>,
    },
    /// Create a new Docker context for a remote host
    Create {
        /// Context name
        name: String,

        /// Docker host address (e.g. tcp://192.168.1.100:2375, ssh://user@host)
        #[arg(long)]
        host: String,

        /// Default Docker API version (optional)
        #[arg(long)]
        docker_api_version: Option<String>,

        /// Skip TLS verification for TCP connections
        #[arg(long)]
        skip_tls_verify: bool,

        /// Path to TLS CA certificate
        #[arg(long)]
        tlscacert: Option<String>,

        /// Path to TLS client certificate
        #[arg(long)]
        tlscert: Option<String>,

        /// Path to TLS client key
        #[arg(long)]
        tlskey: Option<String>,
    },
    /// Remove a Docker context
    Rm {
        /// Context name
        name: String,
    },
}

#[derive(Clone, Subcommand)]
pub enum SwarmCommand {
    /// Initialize a new swarm on this node
    Init {
        /// Advertise address for this node
        #[arg(long)]
        advertise_addr: Option<String>,
    },
    /// Join an existing swarm
    Join {
        /// Join token
        token: String,

        /// Address of a manager node
        #[arg(long)]
        remote: String,

        /// Advertise address for this node
        #[arg(long)]
        advertise_addr: Option<String>,
    },
    /// Leave the swarm
    Leave {
        /// Force leave even if this is the last manager
        #[arg(long)]
        force: bool,
    },
    /// Show or rotate join tokens
    Token {
        /// Show only the worker token
        #[arg(long)]
        worker: bool,

        /// Show only the manager token
        #[arg(long)]
        manager: bool,

        /// Rotate the join tokens
        #[arg(long)]
        rotate: bool,
    },
    /// Show swarm status and node lists
    Status,
}

#[derive(Clone, Subcommand)]
pub enum VaultCommand {
    /// Create a new local vault
    Init,
    /// Show vault status
    Status,
    /// Unlock the vault and show contents
    Unlock,
    /// Change the vault password
    SetKey,
    /// List stored credentials per endpoint
    Ls {
        /// Output format
        #[arg(short, long, value_parser = parse_output_format)]
        output: Option<OutputFormat>,
    },
    /// Store join tokens for an endpoint
    Store {
        /// Endpoint name (as registered with `swarmctl endpoint add`)
        endpoint: String,
        /// Worker join token
        #[arg(long)]
        worker_token: Option<String>,
        /// Manager join token
        #[arg(long)]
        manager_token: Option<String>,
    },
    /// Remove stored credentials for an endpoint
    Rm {
        /// Endpoint name
        endpoint: String,
    },
}

#[derive(Clone, Subcommand)]
pub enum VolumeCommand {
    /// List volumes
    Ls {
        /// Output format
        #[arg(short, long, value_parser = parse_output_format)]
        output: Option<OutputFormat>,
    },
    /// Inspect a volume
    Inspect {
        /// Volume name
        name: String,
        /// Output format
        #[arg(short, long, value_parser = parse_output_format)]
        output: Option<OutputFormat>,
    },
    /// Create a volume
    Create {
        /// Volume name
        name: String,
        /// Volume driver (default: local)
        #[arg(long)]
        driver: Option<String>,
        /// Labels (KEY=VAL)
        #[arg(short, long)]
        labels: Vec<String>,
    },
    /// Remove a volume
    Rm {
        /// Volume name
        name: String,
        /// Force removal
        #[arg(long)]
        force: bool,
    },
}

#[derive(Clone, Subcommand)]
pub enum SystemCommand {
    /// Show Docker system information
    Info {
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
    /// Show Docker disk usage
    Df,
    /// Ping the Docker daemon
    Ping,
}

#[derive(Clone, Subcommand)]
pub enum EndpointCommand {
    /// List registered endpoints
    Ls {
        /// Output format
        #[arg(short, long, value_parser = parse_output_format, default_value = "table")]
        output: OutputFormat,

        /// Emit JSON regardless of --output
        #[arg(long)]
        json: bool,
    },

    /// Register a new endpoint
    Add {
        /// Endpoint name (e.g. prod-eu)
        name: String,

        /// Docker host (tcp://, ssh:// or unix://)
        #[arg(long)]
        host: String,

        /// Human-readable description
        #[arg(long, short = 'd')]
        description: Option<String>,

        /// Logical site, e.g. eu-west
        #[arg(long)]
        site: Option<String>,

        /// Environment: prod, staging, dev, ...
        #[arg(long = "env")]
        environment: Option<String>,

        /// Comma-separated tags
        #[arg(long)]
        tags: Vec<String>,

        /// Path to the TLS CA certificate
        #[arg(long)]
        tlscacert: Option<String>,

        /// Path to the TLS client certificate
        #[arg(long)]
        tlscert: Option<String>,

        /// Path to the TLS client key
        #[arg(long)]
        tlskey: Option<String>,

        /// Make this the default endpoint for fan-out
        #[arg(long)]
        default: bool,

        /// Also create a matching Docker context
        #[arg(long)]
        sync: bool,
    },

    /// Remove an endpoint
    Rm {
        /// Endpoint name
        name: String,

        /// Also remove the matching Docker context
        #[arg(long)]
        sync: bool,
    },

    /// Show one endpoint in detail
    Inspect {
        /// Endpoint name
        name: String,

        /// Output format
        #[arg(short, long, value_parser = parse_output_format, default_value = "table")]
        output: OutputFormat,
    },

    /// Check reachability and swarm state of endpoints
    Health {
        /// Comma-separated endpoint names (default: every endpoint)
        #[arg(long)]
        endpoint: Option<String>,

        /// Output format
        #[arg(short, long, value_parser = parse_output_format, default_value = "table")]
        output: OutputFormat,

        /// Re-probe every 5 seconds
        #[arg(short, long)]
        watch: bool,
    },

    /// Import existing Docker contexts as endpoints
    Import {
        /// Also delete Docker contexts with no matching endpoint
        #[arg(long)]
        prune: bool,
    },

    /// Make an endpoint the default for fan-out
    Use {
        /// Endpoint name
        name: String,
    },

    /// Write Docker contexts for all registered endpoints
    Sync,
}

#[derive(Clone, Subcommand)]
pub enum FailoverCommand {
    /// Show failover status and events
    Status {
        /// Output format
        #[arg(short, long, value_parser = parse_output_format)]
        output: Option<OutputFormat>,
    },
    /// Enable automatic failover
    Enable,
    /// Disable automatic failover
    Disable,
    /// Manually migrate a volume between nodes
    Migrate {
        /// Volume name to migrate
        volume: String,
        /// Source node name or ID
        #[arg(long)]
        from: String,
        /// Target node name or ID
        #[arg(long)]
        to: String,
        /// Output format
        #[arg(short, long, value_parser = parse_output_format)]
        output: Option<OutputFormat>,
    },
}

#[derive(Clone, Subcommand)]
pub enum ConfigCommand {
    /// List configs
    Ls,
    /// Create a config from a file or stdin
    Create {
        /// Config name
        name: String,
        /// Read data from file
        #[arg(short = 'f', long = "from-file")]
        from_file: Option<String>,
        /// Read data from stdin
        #[arg(short = 'i', long)]
        stdin: bool,
    },
    /// Remove a config
    Rm {
        /// Config name
        name: String,
    },
    /// Inspect a config
    Inspect {
        /// Config name
        name: String,
    },
    /// View decoded config data
    View {
        /// Config name
        name: String,
    },
}

#[derive(Clone, Subcommand)]
pub enum RolloutCommand {
    /// View rollout status
    Status {
        /// Service name
        service: String,
    },
    /// View rollout history
    History {
        /// Service name
        service: String,
    },
    /// Pause a service rollout
    Pause {
        /// Service name
        service: String,
    },
    /// Resume a paused service rollout
    Resume {
        /// Service name
        service: String,
    },
    /// Restart a service (force redeploy)
    Restart {
        /// Service name
        service: String,
    },
    /// Roll back a service to its previous version
    Undo {
        /// Service name
        service: String,
    },
}

/// Resource kinds that can be compared across clusters.
///
/// Tasks are excluded on purpose: their ids are per-run, so comparing them
/// across clusters produces spurious diffs.
pub fn resolve_diff_resource(token: &str) -> anyhow::Result<ResourceType> {
    let r = crate::cli::registry::resolve(token)?;
    match r {
        ResourceType::Tasks => {
            anyhow::bail!("tasks cannot be diffed across clusters: their ids are per-run")
        }
        _ => Ok(r),
    }
}

/// The resource kinds `get`, `describe`, `apply` and friends accept.
///
/// The `no`/`svc`/`po`/`net`/`sec`/`cm`/`st` short forms are declared as clap
/// aliases so they actually parse. They were previously documented in BUILD.md
/// but only listed in a hand-written `FromStr` impl that clap never called —
/// clap's `ValueEnum` derive supplies its own parser, so `swarmctl get po`
/// failed with "invalid value" despite the docs.
/// Every resource swarmctl can address.
///
/// This is a *dispatch* enum: the `match` arms in `get.rs`, `delete.rs` and the
/// rest stay exhaustive because of it. How a user's string becomes one of these
/// — plural, singular, short name, `TYPE/NAME`, `.VERSION.GROUP`, comma lists —
/// is entirely [`crate::cli::registry`]'s job. Nothing here is a clap
/// `ValueEnum` any more: that used to duplicate every short name as an `alias`
/// attribute, and clap's "invalid value" error was not kubectl's shape.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ResourceType {
    Nodes,
    Services,
    Tasks,
    Networks,
    Secrets,
    Configs,
    Stacks,
}

impl std::fmt::Display for ResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResourceType::Nodes => write!(f, "nodes"),
            ResourceType::Services => write!(f, "services"),
            ResourceType::Tasks => write!(f, "tasks"),
            ResourceType::Networks => write!(f, "networks"),
            ResourceType::Secrets => write!(f, "secrets"),
            ResourceType::Configs => write!(f, "configs"),
            ResourceType::Stacks => write!(f, "stacks"),
        }
    }
}

#[derive(Clone, Subcommand)]
pub enum TopCommand {
    /// Show processes in service containers
    Service {
        /// Service name
        service: String,

        /// Ps arguments (e.g. "aux")
        #[arg(trailing_var_arg = true)]
        ps_args: Vec<String>,
    },
    /// Show node resource capacity
    Node {
        /// Node name (optional, list all if omitted)
        name: Option<String>,
    },
    /// Show per-container CPU, memory, and network stats
    Stats {
        /// Filter by service name
        #[arg(short, long)]
        service: Option<String>,
    },
}

#[derive(Clone, Subcommand)]
pub enum SetCommand {
    /// Set the image of a service
    Image {
        /// Service name
        service: String,
        /// Container image
        image: String,
    },
    /// Set environment variables on a service
    Env {
        /// Service name
        service: String,
        /// Environment variables (KEY=VAL)
        vars: Vec<String>,
    },
    /// Set the number of replicas for a service
    Replicas {
        /// Service name
        service: String,
        /// Number of replicas
        replicas: u64,
    },
}

/// Resource kinds `logs` accepts.
///
/// Parsed through [`crate::cli::registry`] like every other resource, then
/// narrowed: logs only make sense for a workload, not a network or a secret.
pub fn resolve_log_resource(token: &str) -> anyhow::Result<ResourceType> {
    let r = crate::cli::registry::resolve(token)?;
    match r {
        ResourceType::Services | ResourceType::Tasks => Ok(r),
        _ => anyhow::bail!(
            "logs supports services and tasks, not {}",
            crate::cli::registry::info(r).plural
        ),
    }
}

/// Resource kinds `wait` accepts.
pub fn resolve_wait_resource(token: &str) -> anyhow::Result<ResourceType> {
    let r = crate::cli::registry::resolve(token)?;
    match r {
        ResourceType::Services | ResourceType::Tasks | ResourceType::Nodes => Ok(r),
        _ => anyhow::bail!(
            "wait supports services, tasks and nodes, not {}",
            crate::cli::registry::info(r).plural
        ),
    }
}

impl Cli {
    /// Count verbosity flags in a single argv token.
    ///
    /// Accepts `-v`, `-vv`, `-vvv`, `--verbose`, and `--verbose=vv`. Returns 0 for
    /// anything else, including clusters that mix in other flags (`-vy`), which
    /// clap would reject anyway.
    fn count_verbosity_in_arg(arg: &str) -> u8 {
        let Some(body) = arg.strip_prefix('-') else {
            return 0;
        };
        let body = body.strip_prefix('-').unwrap_or(body);
        let body = body.strip_prefix("verbose").unwrap_or(body);
        // Empty here means a bare `-v` or `--verbose`; a bare `-` counts as nothing.
        if body.is_empty() {
            return if arg.ends_with("verbose") || arg == "-v" {
                1
            } else {
                0
            };
        }
        if body.bytes().all(|b| b == b'v') {
            body.len() as u8
        } else {
            0
        }
    }

    /// Count `-v` flags in the raw argv, before clap parsing.
    ///
    /// Logging has to be configured before any work happens, but `-v` lives on
    /// the parsed `Cli`. Scanning argv directly avoids parsing the whole command
    /// twice and keeps `-v` working even for paths that never build a `Cli`.
    pub fn verbose_count(args: impl IntoIterator<Item = String>) -> u8 {
        args.into_iter()
            .map(|a| Self::count_verbosity_in_arg(&a))
            .sum()
    }

    pub async fn run() -> anyhow::Result<()> {
        let cli = Cli::parse();

        // clap folds `env =` into the flag, so by the time we get here a value
        // may have come from the command line or from DOCKER_HOST. Both are
        // handled identically, which is what makes `--host` override
        // DOCKER_HOST rather than the other way round.
        let explicit_host = cli.host.clone();
        let explicit_context = cli.context.clone().filter(|c| !c.trim().is_empty());

        // `--context` used to win silently: the explicit `--host` and every
        // `--tls*` flag were assembled into a ConnectionConfig and then thrown
        // away by the context branch, so `swarmctl --host tcp://x --context prod`
        // connected to prod with no warning at all. Docker treats this as a
        // conflict, and so do we — a silently ignored flag is worse than an error.
        let tls_requested =
            cli.tlscacert.is_some() || cli.tlscert.is_some() || cli.tlskey.is_some();
        connection_conflict(
            explicit_context.as_deref(),
            explicit_host.as_deref(),
            tls_requested,
        )?;

        if explicit_context.is_some() && tls_requested {
            eprintln!(
                "Warning: --tls* flags are ignored when --context is set; \
                 the context carries its own TLS settings."
            );
        }

        let try_client = || -> anyhow::Result<DockerClient> {
            if let Some(ref ctx_name) = explicit_context {
                DockerClient::with_context(Some(ctx_name))
            } else if explicit_host.is_some()
                || cli.tlscacert.is_some()
                || cli.tlscert.is_some()
                || cli.tlskey.is_some()
            {
                let conn_config = crate::api::client::ConnectionConfig {
                    host: explicit_host.clone(),
                    tlscacert: cli.tlscacert.clone(),
                    tlscert: cli.tlscert.clone(),
                    tlskey: cli.tlskey.clone(),
                    ..Default::default()
                };
                DockerClient::with_config(&conn_config)
            } else {
                DockerClient::with_context(None)
            }
        };

        #[cfg(feature = "tui")]
        let is_dashboard = matches!(cli.command, Commands::Dashboard);
        #[cfg(not(feature = "tui"))]
        let is_dashboard = false;
        let connection_target =
            crate::api::docker_check::get_connection_summary(&cli.host, &cli.context);

        let client = match try_client() {
            Ok(c) => Some(c),
            Err(e) if is_dashboard => {
                eprintln!(
                    "Warning: Docker not available ({}). Dashboard will launch in offline mode.",
                    e
                );
                None
            }
            Err(e) => {
                let docker_installed = crate::api::docker_check::is_docker_installed();
                if docker_installed {
                    eprintln!(
                        "Error: Cannot connect to Docker daemon at {}.",
                        connection_target
                    );
                    eprintln!();
                    eprintln!("  The Docker CLI is installed, but the daemon is not reachable.");
                    eprintln!();
                    eprintln!("  Possible causes:");
                    eprintln!(
                        "    - Docker daemon is not running (try: sudo systemctl start docker)"
                    );
                    eprintln!("    - Incorrect DOCKER_HOST or context configuration");
                    eprintln!("    - SSH connection failed for remote host");
                    eprintln!("    - TLS certificate issues");
                    eprintln!();
                    eprintln!("  To connect to a remote Docker daemon:");
                    eprintln!("    --host tcp://<remote-ip>:2375          (unencrypted)");
                    eprintln!("    --host ssh://user@<remote-host>       (SSH tunnel)");
                    eprintln!("    --context <name>                       (use a Docker context)");
                    eprintln!("    DOCKER_HOST=tcp://<remote-ip>:2375 swarmctl ...");
                    eprintln!();
                    eprintln!("  To create a context for a remote host:");
                    eprintln!(
                        "    docker context create remote --docker host=tcp://<remote-ip>:2375"
                    );
                    eprintln!("    swarmctl context use remote");
                    eprintln!();
                    eprintln!("  Full error: {}", e);
                } else {
                    eprintln!("Error: Docker is not installed or not found in PATH.");
                    eprintln!();
                    eprintln!("  Install Docker: https://docs.docker.com/get-docker/");
                    eprintln!();
                    eprintln!("  If Docker is installed but not in PATH, set DOCKER_HOST:");
                    eprintln!("    export DOCKER_HOST=tcp://<docker-host>:2375");
                }
                return Err(e);
            }
        };

        // The banner is advisory, so it goes to stderr: on stdout it would
        // precede the payload and make `swarmctl get services -o json | jq` fail.
        // It is also skipped entirely for machine-readable output, which avoids
        // a pointless `inspect_swarm` round trip.
        let human_output = !cli.output.is_machine();
        if Self::needs_swarm(&cli.command)
            && human_output
            && let Some(ref client) = client
        {
            let swarm_active = crate::api::swarm::is_swarm_active(client.inner()).await;
            eprintln!("Connected to Docker daemon ({})", connection_target);
            if !swarm_active {
                eprintln!("No swarm detected on this node.");
                eprintln!();
                eprintln!("  To initialize a swarm here:");
                eprintln!("    swarmctl swarm init");
                eprintln!();
                eprintln!("  To join an existing remote swarm:");
                eprintln!("    swarmctl swarm join <token> --remote <manager-ip:2377>");
                eprintln!();
                eprintln!("  Continuing without swarm. Some commands may not work.");
            }
        }

        #[cfg(feature = "tui")]
        if matches!(cli.command, Commands::Dashboard) {
            crate::tui::run_tui(client.as_ref()).await?;
            return Ok(());
        }

        // Local-only command groups run before a Docker connection is required:
        // managing the endpoint registry is most useful precisely when no daemon
        // is reachable, and `endpoint health` builds its own per-endpoint clients.
        match cli.command {
            Commands::Endpoint { command } => {
                Self::dispatch_endpoint(command).await?;
                return Ok(());
            }
            Commands::Context { command } => {
                Self::dispatch_context(command, cli.output.clone()).await?;
                return Ok(());
            }
            Commands::Vault { command } => {
                Self::dispatch_vault(command, cli.output.clone()).await?;
                return Ok(());
            }
            Commands::Completion { shell } => {
                completion::run(shell);
                return Ok(());
            }
            Commands::Version => {
                println!("swarmctl version {}", env!("CARGO_PKG_VERSION"));
                println!("Docker API version: {}", bollard::API_DEFAULT_VERSION);
                return Ok(());
            }
            Commands::Explain { resource } => {
                explain::run(resource).await?;
                return Ok(());
            }
            // Builds its own client: the whole point of `api-resources` is to
            // report what a daemon actually says, and it must degrade to the
            // compiled-in table when none is reachable.
            Commands::ApiResources => {
                match client.as_ref() {
                    Some(c) => api_resources::run(c).await?,
                    None => api_resources::run_offline().await?,
                }
                return Ok(());
            }
            // Only touches the local manifest store, so it must work with no
            // daemon reachable — the same reason the endpoint registry is here.
            Commands::DriftForget { endpoint, all } => {
                drift::forget(endpoint.as_deref(), all, cli.host.as_deref())?;
                return Ok(());
            }
            _ => {}
        }

        let client = client.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Docker daemon is not available. Please install Docker or set DOCKER_HOST."
            )
        })?;

        Self::dispatch(&cli, cli.command.clone(), client).await
    }

    /// Execute a command, honouring `--endpoints` fan-out.
    async fn dispatch(cli: &Cli, command: Commands, client: &DockerClient) -> anyhow::Result<()> {
        // Fan-out replaces the single-endpoint path entirely; the default client
        // is only used when no endpoints were named.
        if let Some(selection) = fanout::requested(cli).transpose()? {
            return fanout::run(cli, command, selection).await;
        }

        Self::dispatch_single(cli, command, client).await
    }

    /// Execute a command against one already-connected client.
    pub(crate) async fn dispatch_single(
        cli: &Cli,
        command: Commands,
        client: &DockerClient,
    ) -> anyhow::Result<()> {
        match command {
            // Dispositioned above, before a Docker connection was required.
            Commands::Endpoint { .. }
            | Commands::Context { .. }
            | Commands::Vault { .. }
            | Commands::Completion { .. }
            | Commands::Version
            | Commands::ApiResources
            | Commands::Explain { .. }
            | Commands::DriftForget { .. } => {
                unreachable!("local-only command groups are handled before the client is built")
            }
            Commands::Get {
                resource,
                name,
                show_labels,
                show_kind,
                no_headers,
                label_columns,
                selector,
                field_selector,
                sort_by,
                output: get_output,
            } => {
                let output = get_output.unwrap_or_else(|| cli.output.clone());
                let requests = crate::cli::registry::parse_request(
                    &resource,
                    &name,
                    &crate::cli::kobject::api_version(),
                )?;
                get::run(
                    client,
                    &requests,
                    output,
                    get::ListOptions {
                        show_labels,
                        show_kind,
                        no_headers,
                        label_columns,
                        selector,
                        field_selector,
                        sort_by,
                        watch: cli.watch,
                    },
                )
                .await?;
            }
            Commands::Describe {
                resource,
                name,
                selector,
            } => {
                let resource = crate::cli::registry::resolve(&resource)?;
                describe::run(client, resource, name, selector, &cli.output).await?;
            }
            Commands::Create {
                resource,
                name,
                filename,
                stdin,
                dry_run,
                output: create_output,
            } => {
                let resource = crate::cli::registry::resolve(&resource)?;
                let output = create_output.unwrap_or_else(|| cli.output.clone());
                create::run(client, resource, name, filename, stdin, dry_run, output).await?;
            }
            Commands::Delete {
                resource,
                name,
                selector,
                dry_run,
                ignore_not_found,
                grace_period,
                timeout,
                wait,
            } => {
                let resource = crate::cli::registry::resolve(&resource)?;
                delete::run(
                    client,
                    resource,
                    name,
                    delete::DeleteOptions {
                        selector,
                        dry_run,
                        ignore_not_found,
                        grace_period,
                        timeout,
                        wait_for_delete: wait,
                        host: cli.host.clone(),
                    },
                )
                .await?;
            }
            Commands::Scale { name, replicas } => {
                scale::run(client, &name, replicas).await?;
            }
            Commands::Logs {
                resource,
                name,
                follow,
                tail,
                previous,
                timestamps,
                since,
                prefix,
                ignore_errors,
            } => {
                let resource = crate::cli::root::resolve_log_resource(&resource)?;
                logs::run(
                    client,
                    resource,
                    name,
                    logs::LogOptions {
                        follow,
                        tail,
                        previous,
                        timestamps,
                        since,
                        prefix,
                        ignore_errors,
                    },
                )
                .await?;
            }
            Commands::Ports {
                #[cfg(feature = "tui")]
                tui,
                available,
                range_start,
                range_end,
                protocol,
            } => {
                #[cfg(feature = "tui")]
                if tui {
                    ports::run_tui(client).await?;
                } else {
                    ports::run(
                        client,
                        cli.output.clone(),
                        available,
                        range_start,
                        range_end,
                        protocol,
                    )
                    .await?;
                }
                #[cfg(not(feature = "tui"))]
                ports::run(
                    client,
                    cli.output.clone(),
                    available,
                    range_start,
                    range_end,
                    protocol,
                )
                .await?;
            }
            Commands::ClusterInfo => {
                cluster::run(client).await?;
            }
            #[cfg(feature = "tui")]
            Commands::Dashboard => {
                crate::tui::run_tui(Some(client)).await?;
            }
            Commands::Apply {
                resource,
                name,
                filename,
                stdin,
                dry_run,
                output: apply_output,
            } => {
                let resource = crate::cli::registry::resolve(&resource)?;
                let output = apply_output.unwrap_or_else(|| cli.output.clone());
                apply::run(
                    client,
                    resource,
                    apply::ApplyOptions {
                        name,
                        filename,
                        stdin,
                        dry_run,
                        output,
                        host: cli.host.as_deref(),
                    },
                )
                .await?;
            }
            Commands::Run {
                name,
                image,
                replicas,
                env,
                labels,
                network,
                publish,
                output,
            } => {
                run::run(
                    client,
                    run::RunOptions {
                        name,
                        image,
                        replicas,
                        env,
                        labels,
                        network,
                        publish,
                        output: output.unwrap_or_else(|| cli.output.clone()),
                    },
                )
                .await?;
            }
            Commands::Events {
                filter_types,
                since,
            } => {
                events::run(client, filter_types, since).await?;
            }
            Commands::Exec { service, command } => {
                exec::run(client, service, command).await?;
            }
            Commands::Attach {
                service,
                interactive,
            } => {
                attach::run(client, service, interactive).await?;
            }
            Commands::Top { command } => match command {
                TopCommand::Service { service, ps_args } => {
                    top::run_service(client, service, ps_args).await?;
                }
                TopCommand::Node { name } => {
                    top::run_node(client, name).await?;
                }
                TopCommand::Stats { service } => {
                    top::run_stats(client, service).await?;
                }
            },
            Commands::PortForward {
                service,
                local_port,
                container_port,
            } => {
                port_forward::run(client, service, local_port, container_port).await?;
            }
            Commands::Cp { source, target } => {
                cp::run(client, source, target).await?;
            }
            Commands::Rollout { command } => match command {
                RolloutCommand::Status { service } => {
                    rollout::run_status(client, service).await?;
                }
                RolloutCommand::History { service } => {
                    rollout::run_history(client, service).await?;
                }
                RolloutCommand::Restart { service } => {
                    rollout::run_restart(client, service).await?;
                }
                RolloutCommand::Undo { service } => {
                    rollout::run_undo(client, service).await?;
                }
                RolloutCommand::Pause { service } => {
                    rollout::run_pause(client, service).await?;
                }
                RolloutCommand::Resume { service } => {
                    rollout::run_resume(client, service).await?;
                }
            },
            Commands::Label {
                resource,
                name,
                labels,
                overwrite,
                all,
                ..
            } => {
                let resource = crate::cli::registry::resolve(&resource)?;
                let name = name.ok_or_else(|| anyhow::anyhow!("name is required for label"))?;
                label::run(client, resource, name, labels, overwrite, all).await?;
            }
            Commands::Set { command } => match command {
                SetCommand::Image { service, image } => {
                    set::run_image(client, service, image).await?;
                }
                SetCommand::Env { service, vars } => {
                    set::run_env(client, service, vars, false).await?;
                }
                SetCommand::Replicas { service, replicas } => {
                    set::run_replicas(client, service, replicas).await?;
                }
            },
            Commands::Drain { name } => {
                cordon::run_drain(client, name).await?;
            }
            Commands::Edit { resource, name } => {
                let resource = crate::cli::registry::resolve(&resource)?;
                // Passed as the typed enum, not a stringified name: the string
                // form could only ever reach the plural arms, leaving the
                // singular ones unreachable.
                edit::run(client, resource, name).await?;
            }
            Commands::Diff {
                resource,
                name,
                filename,
            } => {
                let resource = crate::cli::registry::resolve(&resource)?;
                diff::run(client, resource, name, filename).await?;
            }
            Commands::Drift { output } => {
                let endpoint =
                    crate::api::endpoints::current_endpoint_key_with(cli.host.as_deref());
                let code = drift::run(
                    client,
                    &endpoint,
                    output.unwrap_or_else(|| cli.output.clone()),
                )
                .await?;
                if code != 0 {
                    std::process::exit(code);
                }
            }
            Commands::Patch {
                resource,
                name,
                patch,
                patch_type,
                dry_run,
            } => {
                let resource = crate::cli::registry::resolve(&resource)?;
                let patch_content = if let Some(stripped) = patch.strip_prefix('@') {
                    std::fs::read_to_string(stripped)?
                } else {
                    patch
                };
                patch::run(
                    client,
                    resource,
                    name,
                    patch_content,
                    dry_run,
                    Some(patch_type),
                )
                .await?;
            }
            Commands::Cordon { name } => {
                cordon::run_cordon(client, name).await?;
            }
            Commands::Uncordon { name } => {
                cordon::run_uncordon(client, name).await?;
            }
            Commands::Wait {
                resource,
                name,
                condition,
                timeout,
            } => {
                // Reject unsupported kinds up front rather than after connecting.
                let resource = crate::cli::root::resolve_wait_resource(&resource)?;
                wait::run(client, resource, name, condition, timeout).await?;
            }
            Commands::Stack { command } => match command {
                StackCommand::Deploy { compose_file, name } => {
                    stack::deploy(client, compose_file, name).await?;
                }
                StackCommand::Rm { name } => {
                    stack::remove(client, &name).await?;
                }
                StackCommand::Ls => {
                    stack::list(client).await?;
                }
            },
            Commands::Config { command } => match command {
                ConfigCommand::Ls => {
                    config::run_ls(client, cli.output.clone()).await?;
                }
                ConfigCommand::Create {
                    name,
                    from_file,
                    stdin,
                } => {
                    config::run_create(client, name, from_file, stdin).await?;
                }
                ConfigCommand::Rm { name } => {
                    config::run_rm(client, name).await?;
                }
                ConfigCommand::Inspect { name } => {
                    config::run_inspect(client, name, cli.output.clone()).await?;
                }
                ConfigCommand::View { name } => {
                    config::run_view(client, name).await?;
                }
            },
            Commands::Taint {
                name,
                taints,
                remove,
                overwrite,
            } => {
                taint::run(client, name, taints, remove, overwrite).await?;
            }
            Commands::Replace {
                resource,
                filename,
                stdin,
                dry_run,
                force,
                output: replace_output,
            } => {
                let resource = crate::cli::registry::resolve(&resource)?;
                replace::run(
                    client,
                    resource,
                    filename,
                    stdin,
                    dry_run,
                    force,
                    replace_output.unwrap_or_else(|| cli.output.clone()),
                )
                .await?;
            }
            Commands::DiffClusters {
                left,
                right,
                resource,
                output,
            } => {
                let resource = crate::cli::root::resolve_diff_resource(&resource)?;
                let registry = crate::api::endpoints::Registry::load()?;
                let endpoints = registry.select(&[left, right])?;
                let result = crate::api::diff::diff(&endpoints[0], &endpoints[1], resource).await?;

                match output {
                    OutputFormat::Json => crate::utils::printer::print_json(&result)?,
                    OutputFormat::Yaml => crate::utils::printer::print_yaml(&result)?,
                    _ => print!("{}", crate::api::diff::render_diff(&result)),
                }

                // Non-zero when the clusters differ or a side was unreachable,
                // so a CI job can gate on "these clusters match".
                if !result.identical
                    || result.left_unreachable.is_some()
                    || result.right_unreachable.is_some()
                {
                    std::process::exit(1);
                }
            }
            Commands::Swarm { command } => match command {
                SwarmCommand::Init { advertise_addr } => {
                    swarm::run_init(client, advertise_addr).await?;
                }
                SwarmCommand::Join {
                    token,
                    remote,
                    advertise_addr,
                } => {
                    swarm::run_join(client, token, remote, advertise_addr).await?;
                }
                SwarmCommand::Leave { force } => {
                    swarm::run_leave(client, force).await?;
                }
                SwarmCommand::Token {
                    worker,
                    manager,
                    rotate,
                } => {
                    swarm::run_token(client, worker, manager, rotate).await?;
                }
                SwarmCommand::Status => {
                    swarm::run_status(client).await?;
                }
            },
            Commands::Discover {
                subnet,
                json,
                no_tui,
            } => {
                if json || no_tui {
                    discover::run_scan(client, subnet.clone(), json).await?;
                } else {
                    #[cfg(feature = "tui")]
                    {
                        tui::discover::run_discovery_tui(client, subnet.clone()).await?;
                    }
                    #[cfg(not(feature = "tui"))]
                    {
                        discover::run_interactive(client, subnet.clone()).await?;
                    }
                }
            }
            Commands::Promote { name } => {
                let node_id =
                    crate::api::node::get_node_id_by_hostname(client.inner(), &name).await?;
                let node_id =
                    node_id.ok_or_else(|| anyhow::anyhow!("Node '{}' not found", name))?;
                crate::api::node::promote_node(client.inner(), &node_id).await?;
                println!("Node {} promoted to manager.", name);
            }
            Commands::Demote { name } => {
                let node_id =
                    crate::api::node::get_node_id_by_hostname(client.inner(), &name).await?;
                let node_id =
                    node_id.ok_or_else(|| anyhow::anyhow!("Node '{}' not found", name))?;
                crate::api::node::demote_node(client.inner(), &node_id).await?;
                println!("Node {} demoted to worker.", name);
            }
            Commands::Volume { command } => match command {
                VolumeCommand::Ls { output } => {
                    let output = output.unwrap_or_else(|| cli.output.clone());
                    volume::run_ls(client, output).await?;
                }
                VolumeCommand::Inspect { name, output } => {
                    let output = output.unwrap_or_else(|| cli.output.clone());
                    volume::run_inspect(client, name, output).await?;
                }
                VolumeCommand::Create {
                    name,
                    driver,
                    labels,
                } => {
                    volume::run_create(client, name, driver, labels).await?;
                }
                VolumeCommand::Rm { name, force } => {
                    volume::run_rm(client, name, force).await?;
                }
            },
            Commands::System { command } => match command {
                SystemCommand::Info { json } => system::info(client, json).await?,
                SystemCommand::Df => system::df(client).await?,
                SystemCommand::Ping => system::ping(client).await?,
            },
            Commands::Failover { command } => match command {
                FailoverCommand::Status { output } => {
                    let output = output.unwrap_or_else(|| cli.output.clone());
                    failover::run_status(client, output).await?;
                }
                FailoverCommand::Enable => {
                    failover::run_enable(client).await?;
                }
                FailoverCommand::Disable => {
                    failover::run_disable(client).await?;
                }
                FailoverCommand::Migrate {
                    volume,
                    from,
                    to,
                    output,
                } => {
                    let output = output.unwrap_or_else(|| cli.output.clone());
                    failover::run_migrate(client, volume, from, to, output).await?;
                }
            },
        }

        Ok(())
    }

    /// Endpoint management never needs a Docker connection.
    async fn dispatch_endpoint(command: EndpointCommand) -> anyhow::Result<()> {
        match command {
            EndpointCommand::Ls { output, json } => {
                let output = if json { OutputFormat::Json } else { output };
                endpoint::run_ls(output, json).await?;
            }
            EndpointCommand::Add {
                name,
                host,
                description,
                site,
                environment,
                tags,
                tlscacert,
                tlscert,
                tlskey,
                default,
                sync,
            } => {
                let tls = if tlscacert.is_some() || tlscert.is_some() || tlskey.is_some() {
                    Some(crate::api::context::TlsConfig {
                        ca_file: tlscacert,
                        cert_file: tlscert,
                        key_file: tlskey,
                    })
                } else {
                    None
                };
                endpoint::run_add(endpoint::AddOptions {
                    name,
                    host,
                    description,
                    site,
                    environment,
                    tags,
                    tls,
                    set_default: default,
                    sync_docker: sync,
                })
                .await?;
            }
            EndpointCommand::Rm { name, sync } => endpoint::run_rm(name, sync).await?,
            EndpointCommand::Inspect { name, output } => {
                endpoint::run_inspect(name, output).await?
            }
            EndpointCommand::Health {
                endpoint: names,
                output,
                watch,
            } => endpoint::run_health(names, output, watch).await?,
            EndpointCommand::Import { prune } => endpoint::run_import(prune).await?,
            EndpointCommand::Use { name } => endpoint::run_use(name).await?,
            EndpointCommand::Sync => endpoint::run_sync().await?,
        }
        Ok(())
    }

    /// Docker context management is local-only (it edits ~/.docker).
    async fn dispatch_context(
        command: ContextCommand,
        default_output: OutputFormat,
    ) -> anyhow::Result<()> {
        match command {
            ContextCommand::Ls => context::run_ls(default_output).await?,
            ContextCommand::Use { name } => context::run_use(name).await?,
            ContextCommand::Inspect { name, output } => {
                context::run_inspect(name, output.unwrap_or(default_output)).await?
            }
            ContextCommand::Create {
                name,
                host,
                docker_api_version,
                skip_tls_verify,
                tlscacert,
                tlscert,
                tlskey,
            } => {
                context::run_create(
                    name,
                    host,
                    docker_api_version,
                    skip_tls_verify,
                    tlscacert,
                    tlscert,
                    tlskey,
                )
                .await?
            }
            ContextCommand::Rm { name } => context::run_rm(name).await?,
        }
        Ok(())
    }

    /// The vault is a local encrypted file; no daemon required.
    async fn dispatch_vault(
        command: VaultCommand,
        default_output: OutputFormat,
    ) -> anyhow::Result<()> {
        match command {
            VaultCommand::Init => vault::run_init().await?,
            VaultCommand::Status => vault::run_status().await?,
            VaultCommand::Unlock => vault::run_unlock().await?,
            VaultCommand::SetKey => vault::run_set_key().await?,
            VaultCommand::Ls { output } => vault::run_ls(output.unwrap_or(default_output)).await?,
            VaultCommand::Store {
                endpoint,
                manager_token,
                worker_token,
            } => {
                vault::run_store(
                    &endpoint,
                    worker_token.as_deref(),
                    manager_token.as_deref(),
                    default_output,
                )
                .await?
            }
            VaultCommand::Rm { endpoint } => vault::run_rm(&endpoint).await?,
        }
        Ok(())
    }

    fn needs_swarm(cmd: &Commands) -> bool {
        matches!(
            cmd,
            Commands::Get { .. }
                | Commands::Describe { .. }
                | Commands::Create { .. }
                | Commands::Delete { .. }
                | Commands::Scale { .. }
                | Commands::Logs { .. }
                | Commands::Ports { .. }
                | Commands::ClusterInfo
                | Commands::Apply { .. }
                | Commands::Run { .. }
                | Commands::Events { .. }
                | Commands::Exec { .. }
                | Commands::Attach { .. }
                | Commands::Top { .. }
                | Commands::PortForward { .. }
                | Commands::Cp { .. }
                | Commands::Rollout { .. }
                | Commands::Label { .. }
                | Commands::Set { .. }
                | Commands::Drain { .. }
                | Commands::Edit { .. }
                | Commands::Diff { .. }
                | Commands::Patch { .. }
                | Commands::Cordon { .. }
                | Commands::Uncordon { .. }
                | Commands::Wait { .. }
                | Commands::Stack { .. }
                | Commands::Config { .. }
                | Commands::Taint { .. }
                | Commands::Replace { .. }
                | Commands::Promote { .. }
                | Commands::Demote { .. }
                | Commands::Volume { .. }
                | Commands::System { .. }
                | Commands::Failover { .. }
                | Commands::Swarm { .. }
                | Commands::Discover { .. }
                | Commands::Drift { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands};
    use clap::{CommandFactory, Parser};

    /// Every command must be constructible; clap panics at runtime on invalid
    /// definitions (duplicate shorts, bad value parsers, conflicting args).
    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn help_is_generated_without_docker() {
        match Cli::try_parse_from(["swarmctl", "--help"]) {
            Err(e) => assert_eq!(e.kind(), clap::error::ErrorKind::DisplayHelp),
            Ok(_) => panic!("--help should not produce a parsed command"),
        }
    }

    #[test]
    fn version_flag_is_supported() {
        assert!(Cli::try_parse_from(["swarmctl", "version"]).is_ok());
    }

    #[test]
    fn parses_documented_get_invocations() {
        for argv in [
            vec!["swarmctl", "get", "services"],
            vec!["swarmctl", "get", "nodes"],
            vec!["swarmctl", "get", "services", "-o", "json"],
            vec!["swarmctl", "get", "services", "--selector", "app=web"],
            vec!["swarmctl", "get", "services", "--watch"],
            vec!["swarmctl", "-o", "yaml", "get", "tasks"],
        ] {
            assert!(
                Cli::try_parse_from(&argv).is_ok(),
                "failed to parse: {argv:?}"
            );
        }
    }

    #[test]
    fn parses_mutating_and_group_commands() {
        for argv in [
            vec!["swarmctl", "scale", "web", "3"],
            vec!["swarmctl", "delete", "services", "web"],
            vec!["swarmctl", "describe", "services", "web"],
            vec!["swarmctl", "logs", "service", "web", "--follow"],
            vec!["swarmctl", "stack", "ls"],
            vec!["swarmctl", "context", "ls"],
            vec!["swarmctl", "swarm", "status"],
            vec!["swarmctl", "vault", "status"],
            vec!["swarmctl", "volume", "ls"],
            vec!["swarmctl", "system", "info"],
            vec!["swarmctl", "failover", "status"],
            vec!["swarmctl", "events", "--since", "1h"],
            vec!["swarmctl", "ports", "--available", "--protocol", "tcp"],
        ] {
            assert!(
                Cli::try_parse_from(&argv).is_ok(),
                "failed to parse: {argv:?}"
            );
        }
    }

    #[test]
    fn rejects_invalid_arguments() {
        for argv in [
            vec!["swarmctl", "get", "services", "-o", "notaformat"],
            vec!["swarmctl", "definitely-not-a-command"],
            vec!["swarmctl", "scale"],
        ] {
            assert!(
                Cli::try_parse_from(&argv).is_err(),
                "unexpectedly parsed: {argv:?}"
            );
        }
    }

    #[test]
    fn global_host_and_context_flags_are_accepted() {
        assert!(
            Cli::try_parse_from(["swarmctl", "--host", "tcp://10.0.0.1:2376", "get", "nodes"])
                .is_ok()
        );
        // `--context` is global under its long name. `-c` is deliberately not:
        // `stack deploy -c` already owns that short, and a global short must be
        // unique across every subcommand.
        assert!(Cli::try_parse_from(["swarmctl", "--context", "prod", "get", "nodes"]).is_ok());
        assert!(Cli::try_parse_from(["swarmctl", "get", "nodes", "--context", "prod"]).is_ok());
        // The long form must survive a subcommand that has its own positionals.
        assert!(
            Cli::try_parse_from([
                "swarmctl",
                "delete",
                "services",
                "web",
                "--host",
                "tcp://10.0.0.1:2376"
            ])
            .is_ok()
        );
    }

    /// The bug this pins: `--host` written before a subcommand that takes a
    /// positional NAME used to bind the host string to that positional, so
    /// `swarmctl --host ssh://x delete services web` tried to delete a service
    /// literally named `ssh://x`.
    #[test]
    fn a_global_host_does_not_consume_a_positional_name() {
        let cli = Cli::try_parse_from([
            "swarmctl",
            "--host",
            "ssh://zev",
            "delete",
            "services",
            "web",
        ])
        .expect("must parse");
        match cli.command {
            Commands::Delete { name, .. } => {
                assert_eq!(
                    name.as_deref(),
                    Some("web"),
                    "the host must not become the name"
                );
            }
            _ => panic!("expected Delete"),
        }
    }

    #[test]
    fn endpoint_commands_parse() {
        for argv in [
            vec!["swarmctl", "endpoint", "ls"],
            vec!["swarmctl", "endpoint", "ls", "-o", "json"],
            vec!["swarmctl", "endpoint", "ls", "--json"],
            vec![
                "swarmctl",
                "endpoint",
                "add",
                "prod-eu",
                "--host",
                "ssh://ops@10.8.0.4",
            ],
            vec![
                "swarmctl",
                "endpoint",
                "add",
                "prod-eu",
                "--host",
                "tcp://10.8.0.4:2376",
                "--tlscacert",
                "/ca.pem",
                "--tlscert",
                "/cert.pem",
                "--tlskey",
                "/key.pem",
                "--site",
                "eu-west",
                "--env",
                "prod",
                "--tags",
                "offsite,vpn",
                "--sync",
            ],
            vec!["swarmctl", "endpoint", "rm", "prod-eu", "--sync"],
            vec!["swarmctl", "endpoint", "inspect", "prod-eu"],
            vec!["swarmctl", "endpoint", "health"],
            vec!["swarmctl", "endpoint", "health", "--endpoint", "a,b"],
            vec!["swarmctl", "endpoint", "health", "--watch"],
            vec!["swarmctl", "endpoint", "import"],
            vec!["swarmctl", "endpoint", "import", "--prune"],
            vec!["swarmctl", "endpoint", "use", "prod-eu"],
            vec!["swarmctl", "endpoint", "sync"],
        ] {
            assert!(
                Cli::try_parse_from(&argv).is_ok(),
                "failed to parse: {argv:?}"
            );
        }
    }

    #[test]
    fn endpoint_add_requires_a_host() {
        assert!(Cli::try_parse_from(["swarmctl", "endpoint", "add", "prod-eu"]).is_err());
    }

    #[test]
    fn vault_commands_parse() {
        for argv in [
            vec!["swarmctl", "vault", "init"],
            vec!["swarmctl", "vault", "status"],
            vec!["swarmctl", "vault", "unlock"],
            vec!["swarmctl", "vault", "set-key"],
            vec!["swarmctl", "vault", "ls"],
            vec!["swarmctl", "vault", "ls", "-o", "json"],
            vec![
                "swarmctl",
                "vault",
                "store",
                "prod-eu",
                "--worker-token",
                "w",
                "--manager-token",
                "m",
            ],
            vec!["swarmctl", "vault", "rm", "prod-eu"],
        ] {
            assert!(
                Cli::try_parse_from(&argv).is_ok(),
                "failed to parse: {argv:?}"
            );
        }
        // A store with no token at all is caught at runtime with a clear message,
        // so only the missing endpoint name is a parse error.
        assert!(Cli::try_parse_from(["swarmctl", "vault", "store"]).is_err());
    }

    /// Regression guard: these flags used to parse and then be discarded, so a
    /// script could pass `--namespace prod` (or `--all-endpoints`, `--jsonpath`,
    /// `--all-resources`) and get no effect and no warning.
    #[test]
    fn removed_no_op_flags_are_rejected() {
        for gone in ["--all-endpoints", "--namespace", "-n", "--skip-tls-verify"] {
            assert!(
                Cli::try_parse_from(["swarmctl", gone, "get", "nodes"]).is_err(),
                "{gone} was a no-op and must not be accepted again"
            );
        }
        for argv in [
            vec!["swarmctl", "get", "nodes", "--all-namespaces"],
            vec!["swarmctl", "get", "nodes", "-A"],
            vec!["swarmctl", "wait", "services", "web", "--jsonpath", ".x"],
            vec![
                "swarmctl",
                "label",
                "services",
                "web",
                "app=x",
                "--all-resources",
            ],
            vec!["swarmctl", "explain", "services", "--recursive"],
            vec!["swarmctl", "delete", "services", "web", "--force"],
        ] {
            assert!(
                Cli::try_parse_from(&argv).is_err(),
                "should reject no-op flag: {argv:?}"
            );
        }
    }

    #[test]
    fn create_and_apply_output_are_typed_formats() {
        // These were `Option<String>` and never parsed, so any value was
        // accepted and ignored.
        for argv in [
            vec![
                "swarmctl",
                "create",
                "services",
                "--filename",
                "s.json",
                "-o",
                "json",
            ],
            vec![
                "swarmctl",
                "apply",
                "services",
                "--filename",
                "s.json",
                "-o",
                "yaml",
            ],
        ] {
            assert!(Cli::try_parse_from(&argv).is_ok(), "should parse: {argv:?}");
        }
        assert!(
            Cli::try_parse_from([
                "swarmctl",
                "create",
                "services",
                "--filename",
                "s.json",
                "-o",
                "notaformat"
            ])
            .is_err()
        );
    }

    #[test]
    fn run_publish_and_network_are_usable() {
        for argv in [
            vec![
                "swarmctl",
                "run",
                "web",
                "-i",
                "nginx",
                "--publish",
                "8080:80",
            ],
            vec![
                "swarmctl",
                "run",
                "web",
                "-i",
                "nginx",
                "--publish",
                "5353:53/udp",
                "--network",
                "frontend",
                "--labels",
                "app=web",
            ],
        ] {
            assert!(Cli::try_parse_from(&argv).is_ok(), "should parse: {argv:?}");
        }
    }

    #[test]
    fn fanout_flags_parse_and_depend_on_endpoints() {
        assert!(Cli::try_parse_from(["swarmctl", "--endpoints", "a,b", "get", "nodes"]).is_ok());
        assert!(Cli::try_parse_from(["swarmctl", "get", "nodes", "--endpoints", "a,b"]).is_ok());
        // The removed no-op flags must stay gone.
        for gone in ["--all-endpoints", "--namespace", "--skip-tls-verify"] {
            assert!(
                Cli::try_parse_from(["swarmctl", gone, "get", "nodes"]).is_err(),
                "{gone} was a no-op and should not have come back"
            );
        }
        assert!(Cli::try_parse_from(["swarmctl", "-y", "get", "nodes"]).is_ok());
    }

    #[test]
    fn diff_clusters_parses() {
        for argv in [
            vec!["swarmctl", "diff-clusters", "a", "b"],
            vec![
                "swarmctl",
                "diff-clusters",
                "a",
                "b",
                "--resource",
                "services",
            ],
            vec![
                "swarmctl",
                "diff-clusters",
                "a",
                "b",
                "--resource",
                "nodes",
                "-o",
                "json",
            ],
        ] {
            assert!(
                Cli::try_parse_from(&argv).is_ok(),
                "failed to parse: {argv:?}"
            );
        }
        assert!(Cli::try_parse_from(["swarmctl", "diff-clusters", "a"]).is_err());
        // Tasks are rejected by the resolver rather than by clap, since
        // `--resource` is now a plain token.
        assert!(
            crate::cli::root::resolve_diff_resource("tasks").is_err(),
            "tasks are not comparable across clusters"
        );
        assert!(crate::cli::root::resolve_diff_resource("services").is_ok());
    }

    #[test]
    fn patch_type_is_forwarded_and_validated_at_runtime() {
        // Accepted by clap, rejected by the handler with a clear message.
        assert!(
            Cli::try_parse_from([
                "swarmctl",
                "patch",
                "services",
                "web",
                "--patch",
                "{}",
                "--patch-type",
                "json"
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "swarmctl",
                "patch",
                "services",
                "web",
                "--patch",
                "{}",
                "--dry-run"
            ])
            .is_ok()
        );
    }

    /// `explain` and `api-resources` read the registry, so they can no longer
    /// drift from what the parser accepts. This asserts the invariant that the
    /// registry is the single source of truth.
    #[test]
    fn explain_and_api_resources_come_from_the_registry() {
        use crate::cli::explain;
        use crate::cli::registry;

        for info in registry::REGISTRY {
            assert!(
                !info.plural.is_empty(),
                "{:?} has no plural name",
                info.resource
            );
            assert!(!info.singular.is_empty(), "{} has no singular", info.plural);
            assert!(
                !info.short_names.is_empty(),
                "{} has no short name",
                info.plural
            );
            for short in info.short_names {
                assert!(!short.contains(' '), "{short} contains a space");
            }
            // Every spelling must round-trip through the resolver.
            for spelling in registry::all_spellings_for(info) {
                assert_eq!(
                    registry::resolve(spelling).unwrap(),
                    info.resource,
                    "{spelling} did not resolve back"
                );
            }
            for short in info.short_names {
                assert_eq!(registry::resolve(short).unwrap(), info.resource);
            }
            assert!(!explain::fields_for(info.resource).is_empty());
        }
    }

    /// Every spelling the registry advertises must actually work on the CLI.
    #[test]
    fn every_documented_spelling_parses() {
        use crate::cli::registry;

        for spelling in registry::all_spellings() {
            let argv = vec!["swarmctl", "get", spelling];
            let cli = Cli::try_parse_from(&argv)
                .unwrap_or_else(|e| panic!("`get {spelling}` should parse: {e}"));
            match cli.command {
                Commands::Get { resource, .. } => {
                    let parsed = registry::parse_request(&resource, &[], "v1.52")
                        .unwrap_or_else(|e| panic!("`get {spelling}` failed to resolve: {e}"));
                    assert_eq!(parsed.len(), 1);
                }
                _ => panic!("expected a Get command"),
            }
        }
    }

    /// `--context` and `--host` used to be accepted together with the host
    /// silently discarded. They are now a hard conflict.
    #[test]
    fn context_and_host_together_are_a_conflict() {
        let err =
            crate::cli::root::connection_conflict(Some("prod"), Some("tcp://10.0.0.1:2375"), false)
                .unwrap_err()
                .to_string();
        assert!(err.contains("cannot combine"), "{err}");
        assert!(err.contains("prod"), "{err}");
    }

    #[test]
    fn context_alone_is_not_a_conflict() {
        assert!(crate::cli::root::connection_conflict(Some("prod"), None, false).is_ok());
        assert!(crate::cli::root::connection_conflict(None, Some("tcp://x"), false).is_ok());
        assert!(crate::cli::root::connection_conflict(None, None, true).is_ok());
    }

    /// `logs` and `wait` narrow the shared registry rather than carrying their
    /// own duplicate alias lists.
    #[test]
    fn logs_and_wait_narrow_the_shared_registry() {
        use crate::cli::root::{resolve_log_resource, resolve_wait_resource};

        for alias in ["svc", "services", "service", "po", "tasks", "no", "nodes"] {
            assert!(resolve_wait_resource(alias).is_ok(), "wait {alias}");
        }
        for alias in ["svc", "services", "service", "po", "tasks"] {
            assert!(resolve_log_resource(alias).is_ok(), "logs {alias}");
        }
        assert!(
            resolve_log_resource("net").is_err(),
            "logs must reject networks"
        );
        assert!(
            resolve_wait_resource("net").is_err(),
            "wait must reject networks"
        );
    }

    #[test]
    fn shell_completions_generate_for_all_shells() {
        use clap_complete::Shell;
        let mut cmd = Cli::command();
        let app_name = cmd.get_name().to_string();
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
            let mut buf = Vec::new();
            clap_complete::generate(shell, &mut cmd, app_name.clone(), &mut buf);
            assert!(!buf.is_empty(), "no completions generated for {shell}");
        }
    }

    #[test]
    fn swarm_only_commands_are_recognised() {
        for argv in [
            vec!["swarmctl", "get", "nodes"],
            vec!["swarmctl", "stack", "ls"],
            vec!["swarmctl", "swarm", "status"],
        ] {
            let cli = Cli::try_parse_from(&argv).unwrap();
            assert!(
                Cli::needs_swarm(&cli.command),
                "expected swarm requirement: {argv:?}"
            );
        }
        for argv in [
            vec!["swarmctl", "version"],
            vec!["swarmctl", "completion", "bash"],
        ] {
            let cli = Cli::try_parse_from(&argv).unwrap();
            assert!(
                !Cli::needs_swarm(&cli.command),
                "expected no swarm requirement: {argv:?}"
            );
        }
        // Ensure the enum stays exercised so new variants get classified.
        let _ = std::mem::discriminant(&Commands::Version);
    }

    /// `-v` used to be declared, parsed and read nowhere, so `-vv` did nothing
    /// and env_logger's `error` default hid every warning the codebase emits.
    #[test]
    fn verbosity_flags_are_counted_before_parsing() {
        let v = |args: &[&str]| Cli::verbose_count(args.iter().map(|s| s.to_string()));

        assert_eq!(v(&[]), 0);
        assert_eq!(v(&["-v"]), 1);
        assert_eq!(v(&["-vv"]), 2);
        assert_eq!(v(&["-vvv"]), 3);
        assert_eq!(v(&["--verbose"]), 1);
        // `=value` is not accepted by clap's Count action, so it must not be
        // counted as verbosity either.
        assert_eq!(v(&["--verbose=vv"]), 0);

        // Anywhere in the command line, including after the subcommand.
        assert_eq!(v(&["get", "services", "-vv"]), 2);
        assert_eq!(v(&["-v", "get", "services", "-v"]), 2);

        // Unrelated arguments contribute nothing.
        assert_eq!(v(&["get", "services", "-o", "json"]), 0);
        assert_eq!(v(&["get", "services", "--watch"]), 0);
        assert_eq!(v(&["-y"]), 0);
        assert_eq!(v(&["-w"]), 0);
    }

    #[test]
    fn verbose_is_accepted_in_either_position() {
        for argv in [
            vec!["swarmctl", "-vv", "get", "services"],
            vec!["swarmctl", "get", "services", "-vv"],
            vec!["swarmctl", "--verbose", "get", "services"],
        ] {
            let cli =
                Cli::try_parse_from(&argv).unwrap_or_else(|e| panic!("{argv:?} should parse: {e}"));
            let expected: u8 = argv.iter().map(|a| Cli::count_verbosity_in_arg(a)).sum();
            assert_eq!(cli.verbose, expected, "{argv:?}");
        }
    }
}
