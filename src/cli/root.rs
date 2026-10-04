use crate::api::client::DockerClient;
use crate::cli::{
    api_resources, apply, attach, cluster, completion, config, context, cordon, cp, create, delete,
    describe, diff, discover, edit, endpoint, events, exec, explain, failover, fanout, get, label,
    logs, patch, port_forward, ports, replace, rollout, run, scale, set, stack, swarm, system,
    taint, top, vault, volume, wait,
};
#[cfg(feature = "tui")]
use crate::tui;
use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

#[derive(Parser, Clone)]
#[command(name = "swarmctl")]
#[command(about = "Docker Swarm control plane CLI")]
#[command(long_about = "A kubectl-inspired CLI for managing Docker Swarm clusters")]
pub struct Cli {
    /// Output format
    #[arg(short, long, value_enum, default_value = "table")]
    pub(crate) output: OutputFormat,

    /// Watch for changes
    // Global so it works in either position: `swarmctl --watch get services`
    // and `swarmctl get services --watch`.
    #[arg(short, long, global = true)]
    watch: bool,

    /// Namespace (for future use with stacks)
    #[arg(short, long)]
    namespace: Option<String>,

    /// Docker daemon host (e.g. tcp://192.168.1.100:2376, ssh://user@host, unix:///var/run/docker.sock)
    #[arg(long, env = "DOCKER_HOST")]
    host: Option<String>,

    /// Docker context to use (overrides DOCKER_HOST and current context)
    #[arg(long, short = 'c', env = "DOCKER_CONTEXT")]
    context: Option<String>,

    /// Path to TLS CA certificate
    #[arg(long)]
    tlscacert: Option<String>,

    /// Path to TLS client certificate
    #[arg(long)]
    tlscert: Option<String>,

    /// Path to TLS client key
    #[arg(long)]
    tlskey: Option<String>,

    /// Skip TLS certificate verification (rejected: certificates are always verified)
    #[arg(long)]
    skip_tls_verify: bool,

    /// Comma-separated endpoint names to run this command against
    ///
    /// Registered with `swarmctl endpoint add`. Runs the command against every
    /// listed endpoint and combines the results.
    #[arg(long, global = true, value_name = "NAMES")]
    pub(crate) endpoints: Option<String>,

    /// With --endpoints, include endpoints that are not marked default
    #[arg(long, global = true, requires = "endpoints")]
    pub(crate) all_endpoints: bool,

    /// Assume yes to confirmation prompts (required for write fan-out in scripts)
    #[arg(short = 'y', long, global = true)]
    pub(crate) yes: bool,

    /// Verbose output level
    #[arg(short = 'v', global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
    Wide,
    Name,
}

#[derive(Clone, Subcommand)]
pub(crate) enum Commands {
    /// Display one or many resources
    Get {
        /// Resource type (can be comma-separated: svc,po)
        resource: ResourceType,

        /// Resource name
        name: Option<String>,

        /// Show labels
        #[arg(long)]
        show_labels: bool,

        /// Filter by label selector
        #[arg(long)]
        selector: Option<String>,

        /// Filter by field selector (key=value)
        #[arg(long)]
        field_selector: Option<String>,

        /// Sort by a JSON field path (e.g. .metadata.name)
        #[arg(long)]
        sort_by: Option<String>,

        /// Include all namespaces
        #[arg(short = 'A', long)]
        all_namespaces: bool,

        /// Output format (table, json, yaml, wide, name)
        #[arg(short, long, value_enum)]
        output: Option<OutputFormat>,
    },

    /// Show detailed information about a resource
    Describe {
        /// Resource type
        resource: ResourceType,

        /// Resource name (optional if using --selector)
        name: Option<String>,

        /// Filter by label selector
        #[arg(long)]
        selector: Option<String>,
    },

    /// Create a resource from a file or stdin
    Create {
        /// Resource type
        resource: ResourceType,

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

        /// Output format for dry run
        #[arg(long)]
        output: Option<String>,
    },

    /// Delete resources
    Delete {
        /// Resource type
        resource: ResourceType,

        /// Resource name
        name: Option<String>,

        /// Selector to filter resources
        #[arg(long)]
        selector: Option<String>,

        /// Force deletion
        #[arg(long)]
        force: bool,

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
        resource: LogResourceType,

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
        /// Resource type
        resource: ResourceType,

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

        /// Output format for dry run
        #[arg(long)]
        output: Option<String>,
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

        /// Publish a port (container-port or host-port:container-port)
        #[arg(short, long)]
        publish: Vec<String>,
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
        resource: ResourceType,

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

        /// Operate on all resources of the specified type
        #[arg(long)]
        all_resources: bool,

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
        /// Resource type
        resource: Option<ResourceType>,

        /// Show nested fields recursively
        #[arg(short, long)]
        recursive: bool,

        /// API version to use for explanation
        #[arg(long)]
        api_version: Option<String>,
    },

    /// Drain a node (set to drain availability)
    Drain {
        /// Node name
        name: String,
    },

    /// Edit a resource
    Edit {
        /// Resource type (service, node, secret, config)
        resource: ResourceType,

        /// Resource name
        name: String,
    },

    /// Diff a resource against a file
    Diff {
        /// Resource type (service, node, secret, config)
        resource: ResourceType,

        /// Resource name
        name: String,

        /// File to diff against
        #[arg(short, long)]
        filename: String,
    },

    /// Apply a JSON merge patch to a resource
    Patch {
        /// Resource type (service, node, secret, config)
        resource: ResourceType,

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
        /// Resource type
        resource: WaitResourceType,

        /// Resource name
        name: String,

        /// Condition to wait for
        condition: String,

        /// Timeout in seconds
        #[arg(short, long, default_value = "60")]
        timeout: u64,

        /// JSONPath expression to wait for
        #[arg(long)]
        jsonpath: Option<String>,
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
        /// Resource type
        resource: ResourceType,

        /// Filename to read from
        #[arg(short, long)]
        filename: Option<String>,

        /// Create from stdin
        #[arg(short = 'i')]
        stdin: bool,

        /// Dry run mode
        #[arg(long)]
        dry_run: bool,

        /// Force replace (delete and recreate)
        #[arg(long)]
        force: bool,
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
        #[arg(long, value_enum, default_value = "services")]
        resource: DiffResourceType,

        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
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
        #[arg(short, long, value_enum)]
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
        #[arg(short, long, value_enum)]
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
        #[arg(short, long, value_enum)]
        output: Option<OutputFormat>,
    },
    /// Inspect a volume
    Inspect {
        /// Volume name
        name: String,
        /// Output format
        #[arg(short, long, value_enum)]
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
        #[arg(short, long, value_enum, default_value = "table")]
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
        #[arg(short, long, value_enum, default_value = "table")]
        output: OutputFormat,
    },

    /// Check reachability and swarm state of endpoints
    Health {
        /// Comma-separated endpoint names (default: every endpoint)
        #[arg(long)]
        endpoint: Option<String>,

        /// Output format
        #[arg(short, long, value_enum, default_value = "table")]
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
        #[arg(short, long, value_enum)]
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
        #[arg(short, long, value_enum)]
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
/// A separate enum from [`ResourceType`] so `--help` lists only valid choices
/// and clap rejects `tasks` up front, rather than the handler explaining why
/// afterwards.
#[derive(Clone, Copy, PartialEq, Eq, Hash, ValueEnum, Debug)]
pub enum DiffResourceType {
    Nodes,
    Services,
    Networks,
    Secrets,
    Configs,
    Stacks,
}

impl From<DiffResourceType> for ResourceType {
    fn from(value: DiffResourceType) -> Self {
        match value {
            DiffResourceType::Nodes => ResourceType::Nodes,
            DiffResourceType::Services => ResourceType::Services,
            DiffResourceType::Networks => ResourceType::Networks,
            DiffResourceType::Secrets => ResourceType::Secrets,
            DiffResourceType::Configs => ResourceType::Configs,
            DiffResourceType::Stacks => ResourceType::Stacks,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, ValueEnum, Debug)]
pub enum ResourceType {
    Nodes,
    Services,
    Tasks,
    Networks,
    Secrets,
    Configs,
    Stacks,
}

impl std::str::FromStr for ResourceType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "no" | "node" | "nodes" => Ok(ResourceType::Nodes),
            "svc" | "service" | "services" => Ok(ResourceType::Services),
            "po" | "task" | "tasks" => Ok(ResourceType::Tasks),
            "net" | "network" | "networks" => Ok(ResourceType::Networks),
            "sec" | "secret" | "secrets" => Ok(ResourceType::Secrets),
            "cm" | "config" | "configs" => Ok(ResourceType::Configs),
            "st" | "stack" | "stacks" => Ok(ResourceType::Stacks),
            _ => Err(format!("Unknown resource type: {}", s)),
        }
    }
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

#[derive(Clone, ValueEnum)]
pub enum LogResourceType {
    Service,
    Task,
}

#[derive(Clone, ValueEnum)]
pub enum WaitResourceType {
    Service,
    Task,
    Node,
}

impl std::fmt::Display for WaitResourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WaitResourceType::Service => write!(f, "service"),
            WaitResourceType::Task => write!(f, "task"),
            WaitResourceType::Node => write!(f, "node"),
        }
    }
}

impl Cli {
    pub async fn run() -> anyhow::Result<()> {
        let cli = Cli::parse();

        let try_client = || -> anyhow::Result<DockerClient> {
            if cli.context.is_some()
                || cli.host.is_some()
                || cli.tlscacert.is_some()
                || cli.tlscert.is_some()
                || cli.tlskey.is_some()
                || cli.skip_tls_verify
            {
                let conn_config = crate::api::client::ConnectionConfig {
                    host: cli.host.clone(),
                    tlscacert: cli.tlscacert.clone(),
                    tlscert: cli.tlscert.clone(),
                    tlskey: cli.tlskey.clone(),
                    skip_tls_verify: cli.skip_tls_verify,
                };
                if let Some(ref ctx_name) = cli.context {
                    DockerClient::with_context(Some(ctx_name))
                } else {
                    DockerClient::with_config(&conn_config)
                }
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
        let human_output = matches!(
            cli.output,
            OutputFormat::Table | OutputFormat::Wide | OutputFormat::Name
        );
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
                Self::dispatch_context(command, cli.output).await?;
                return Ok(());
            }
            Commands::Vault { command } => {
                Self::dispatch_vault(command, cli.output).await?;
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
            Commands::ApiResources => {
                api_resources::run().await?;
                return Ok(());
            }
            Commands::Explain {
                resource,
                recursive: _,
                api_version: _,
            } => {
                explain::run(resource.map(|r| r.to_string())).await?;
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
            | Commands::Explain { .. } => {
                unreachable!("local-only command groups are handled before the client is built")
            }
            Commands::Get {
                resource,
                name,
                show_labels,
                selector,
                field_selector,
                sort_by,
                all_namespaces: _,
                output: get_output,
            } => {
                let output = get_output.unwrap_or(cli.output);
                get::run(
                    client,
                    resource,
                    name,
                    output,
                    get::ListOptions {
                        show_labels,
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
                describe::run(client, resource, name, selector, cli.output).await?;
            }
            Commands::Create {
                resource,
                name,
                filename,
                stdin,
                dry_run,
                output: _,
            } => {
                create::run(client, resource, name, filename, stdin, dry_run).await?;
            }
            Commands::Delete {
                resource,
                name,
                selector,
                force,
                dry_run,
                ignore_not_found,
                grace_period,
                timeout,
                wait,
            } => {
                delete::run(
                    client,
                    resource,
                    name,
                    delete::DeleteOptions {
                        selector,
                        force,
                        dry_run,
                        ignore_not_found,
                        grace_period,
                        timeout,
                        wait_for_delete: wait,
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
                        cli.output,
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
                    cli.output,
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
                output: _,
            } => {
                apply::run(client, resource, name, filename, stdin, dry_run).await?;
            }
            Commands::Run {
                name,
                image,
                replicas,
                env,
                labels,
                network: _,
                publish,
            } => {
                run::run(client, name, image, replicas, env, labels, publish).await?;
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
                let resource_str = resource.to_string();
                let name = name.ok_or_else(|| anyhow::anyhow!("name is required for label"))?;
                label::run(client, &resource_str, name, labels, overwrite, all).await?;
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
                let resource_str = resource.to_string();
                edit::run(client, &resource_str, name).await?;
            }
            Commands::Diff {
                resource,
                name,
                filename,
            } => {
                let resource_str = resource.to_string();
                diff::run(client, &resource_str, name, filename).await?;
            }
            Commands::Patch {
                resource,
                name,
                patch,
                patch_type,
                dry_run,
            } => {
                let resource_str = resource.to_string();
                let patch_content = if let Some(stripped) = patch.strip_prefix('@') {
                    std::fs::read_to_string(stripped)?
                } else {
                    patch
                };
                patch::run(
                    client,
                    &resource_str,
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
                jsonpath: _,
            } => {
                // Reject unsupported kinds up front rather than after connecting.
                let resource_str = resource.to_string();
                if !wait::resource_type_supported(resource) {
                    anyhow::bail!(
                        "wait is not supported for {}; supported kinds: services, tasks, nodes",
                        resource_str.to_lowercase()
                    );
                }
                wait::run(client, &resource_str, name, condition, timeout).await?;
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
                    config::run_ls(client, cli.output).await?;
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
                    config::run_inspect(client, name, cli.output).await?;
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
            } => {
                replace::run(client, resource, filename, stdin, dry_run, force).await?;
            }
            Commands::DiffClusters {
                left,
                right,
                resource,
                output,
            } => {
                let registry = crate::api::endpoints::Registry::load()?;
                let endpoints = registry.select(&[left, right])?;
                let result =
                    crate::api::diff::diff(&endpoints[0], &endpoints[1], resource.into()).await?;

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
                    let output = output.unwrap_or(cli.output);
                    volume::run_ls(client, output).await?;
                }
                VolumeCommand::Inspect { name, output } => {
                    let output = output.unwrap_or(cli.output);
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
                    let output = output.unwrap_or(cli.output);
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
                    let output = output.unwrap_or(cli.output);
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
        assert!(Cli::try_parse_from(["swarmctl", "-c", "prod", "get", "nodes"]).is_ok());
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

    #[test]
    fn fanout_flags_parse_and_depend_on_endpoints() {
        assert!(Cli::try_parse_from(["swarmctl", "--endpoints", "a,b", "get", "nodes"]).is_ok());
        assert!(Cli::try_parse_from(["swarmctl", "get", "nodes", "--endpoints", "a,b"]).is_ok());
        // --all-endpoints is meaningless without --endpoints.
        assert!(Cli::try_parse_from(["swarmctl", "--all-endpoints", "get", "nodes"]).is_err());
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
        assert!(
            Cli::try_parse_from(["swarmctl", "diff-clusters", "a", "b", "--resource", "tasks"])
                .is_err(),
            "tasks are not comparable, so clap should not offer them"
        );
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
}
