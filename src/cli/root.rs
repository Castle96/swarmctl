use crate::api::client::DockerClient;
use crate::cli::{
    api_resources, apply, attach, cluster, completion, config, context, cordon, cp, create, delete,
    describe, diff, discover, edit, events, exec, explain, get, label, logs, patch, port_forward,
    ports, replace, rollout, run, scale, set, stack, swarm, taint, top, vault, wait,
};
#[cfg(feature = "tui")]
use crate::tui;
use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

#[derive(Parser)]
#[command(name = "swarmctl")]
#[command(about = "Docker Swarm control plane CLI")]
#[command(long_about = "A kubectl-inspired CLI for managing Docker Swarm clusters")]
pub struct Cli {
    /// Output format
    #[arg(short, long, value_enum, default_value = "table")]
    output: OutputFormat,

    /// Watch for changes
    #[arg(short, long)]
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

    /// Verbose output level
    #[arg(short = 'v', global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
    Wide,
    Name,
}

#[derive(Subcommand)]
enum Commands {
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
    Events,

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

        /// Patch type (merge, json, strategic)
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
}

#[derive(Subcommand)]
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

#[derive(Subcommand)]
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
}

#[derive(Subcommand)]
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

#[derive(Subcommand)]
pub enum VaultCommand {
    /// Create a new local vault
    Init,
    /// Show vault status
    Status,
    /// Unlock the vault and show contents
    Unlock,
    /// Change the vault password
    SetKey,
}

#[derive(Subcommand)]
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

#[derive(Subcommand)]
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

#[derive(Clone, ValueEnum, Debug)]
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

#[derive(Subcommand)]
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

#[derive(Subcommand)]
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

        let client = if cli.context.is_some() || cli.host.is_some() || cli.tlscacert.is_some() || cli.tlscert.is_some() || cli.tlskey.is_some() {
            let conn_config = crate::api::client::ConnectionConfig {
                host: cli.host.clone(),
                tlscacert: cli.tlscacert.clone(),
                tlscert: cli.tlscert.clone(),
                tlskey: cli.tlskey.clone(),
            };
            if let Some(ref ctx_name) = cli.context {
                DockerClient::with_context(Some(ctx_name))?
            } else {
                DockerClient::with_config(&conn_config)?
            }
        } else {
            DockerClient::with_context(None)?
        };

        if Self::needs_swarm(&cli.command) {
            if !crate::api::swarm::is_swarm_active(client.inner()).await {
                println!("No swarm detected on this node.");
                print!("Would you like to initialize a swarm? [y/N] ");
                use std::io::Write;
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if input.trim().eq_ignore_ascii_case("y") {
                    let addr = Self::detect_local_ip();
                    println!("Initializing swarm with advertise address {}...", addr);
                    let node_id = crate::api::swarm::init_swarm(client.inner(), &addr).await?;
                    println!("Swarm initialized. Node ID: {}", node_id);

                    let tokens = crate::api::swarm::get_join_tokens(client.inner()).await?;

                    match rpassword::prompt_password("Vault password (to save tokens, or empty to skip): ") {
                        Ok(vault_password) if !vault_password.is_empty() => {
                            let vault = if crate::vault::LocalVault::exists() {
                                crate::vault::LocalVault::open(&vault_password)?
                            } else {
                                crate::vault::LocalVault::create(&vault_password)?
                            };
                            let mut vault = vault;
                            let host = std::env::var("DOCKER_HOST")
                                .unwrap_or_else(|_| "unix:///var/run/docker.sock".to_string());
                            let swarm_name = client.inner().info().await
                                .ok()
                                .and_then(|i| i.name)
                                .unwrap_or_else(|| "docker".to_string());
                            vault.store_swarm_tokens(tokens, None, &host, &swarm_name)?;
                            println!("Tokens saved to vault.");
                        }
                        _ => {
                            println!("Tokens not saved.");
                            println!("  Worker token:  {}...", &tokens.worker[..24.min(tokens.worker.len())]);
                            println!("  Manager token: {}...", &tokens.manager[..24.min(tokens.manager.len())]);
                        }
                    }
                } else {
                    println!("Continuing without swarm. Some commands may not work.");
                }
            }
        }

        match cli.command {
            Commands::Get {
                resource,
                name,
                show_labels,
                selector,
                field_selector,
                sort_by,
                all_namespaces,
                output: get_output,
            } => {
                let output = get_output.unwrap_or(cli.output);
                get::run(
                    &client,
                    resource,
                    name,
                    output,
                    show_labels,
                    selector,
                    field_selector,
                    sort_by,
                    all_namespaces,
                    cli.watch,
                )
                .await?;
            }
            Commands::Describe {
                resource,
                name,
                selector,
            } => {
                describe::run(&client, resource, name, selector, cli.output).await?;
            }
            Commands::Create {
                resource,
                name,
                filename,
                stdin,
                dry_run,
                output: _,
            } => {
                create::run(&client, resource, name, filename, stdin, dry_run).await?;
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
                    &client,
                    resource,
                    name,
                    selector,
                    force,
                    dry_run,
                    ignore_not_found,
                    grace_period,
                    timeout,
                    wait,
                )
                .await?;
            }
            Commands::Scale { name, replicas } => {
                scale::run(&client, &name, replicas).await?;
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
                    &client,
                    resource,
                    name,
                    follow,
                    tail,
                    previous,
                    timestamps,
                    since,
                    prefix,
                    ignore_errors,
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
                    ports::run_tui(&client).await?;
                } else {
                    ports::run(
                        &client,
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
                    &client,
                    cli.output,
                    available,
                    range_start,
                    range_end,
                    protocol,
                )
                .await?;
            }
            Commands::ClusterInfo => {
                cluster::run(&client).await?;
            }
            #[cfg(feature = "tui")]
            Commands::Dashboard => {
                crate::tui::run_tui(&client).await?;
            }
            Commands::Context { command } => match command {
                ContextCommand::Ls => {
                    context::run_ls(cli.output).await?;
                }
                ContextCommand::Use { name } => {
                    context::run_use(name).await?;
                }
                ContextCommand::Inspect { name, output } => {
                    let output = output.unwrap_or(cli.output);
                    context::run_inspect(name, output).await?;
                }
            },
            Commands::Apply {
                resource,
                name,
                filename,
                stdin,
                dry_run,
                output: _,
            } => {
                apply::run(&client, resource, name, filename, stdin, dry_run).await?;
            }
            Commands::Run {
                name,
                image,
                replicas,
                env,
                labels,
                network,
                publish,
            } => {
                run::run(
                    &client, name, image, replicas, env, labels, network, publish,
                )
                .await?;
            }
            Commands::Events => {
                events::run(&client).await?;
            }
            Commands::Exec { service, command } => {
                exec::run(&client, service, command).await?;
            }
            Commands::Attach {
                service,
                interactive,
            } => {
                attach::run(&client, service, interactive).await?;
            }
            Commands::Top { command } => match command {
                TopCommand::Service { service, ps_args } => {
                    top::run_service(&client, service, ps_args).await?;
                }
                TopCommand::Node { name } => {
                    top::run_node(&client, name).await?;
                }
                TopCommand::Stats { service } => {
                    top::run_stats(&client, service).await?;
                }
            },
            Commands::PortForward {
                service,
                local_port,
                container_port,
            } => {
                port_forward::run(&client, service, local_port, container_port).await?;
            }
            Commands::Cp { source, target } => {
                cp::run(&client, source, target).await?;
            }
            Commands::Rollout { command } => match command {
                RolloutCommand::Status { service } => {
                    rollout::run_status(&client, service).await?;
                }
                RolloutCommand::History { service } => {
                    rollout::run_history(&client, service).await?;
                }
                RolloutCommand::Restart { service } => {
                    rollout::run_restart(&client, service).await?;
                }
                RolloutCommand::Undo { service } => {
                    rollout::run_undo(&client, service).await?;
                }
                RolloutCommand::Pause { service } => {
                    rollout::run_pause(&client, service).await?;
                }
                RolloutCommand::Resume { service } => {
                    rollout::run_resume(&client, service).await?;
                }
            },
            Commands::Version => {
                println!(
                    "swarmctl version {}",
                    env!("CARGO_PKG_VERSION")
                );
                println!("Docker API version: {}", bollard::API_DEFAULT_VERSION);
            }
            Commands::Completion { shell } => {
                completion::run(shell);
            }
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
                label::run(
                    &client,
                    &resource_str,
                    name,
                    labels,
                    overwrite,
                    all,
                )
                .await?;
            }
            Commands::Set { command } => match command {
                SetCommand::Image { service, image } => {
                    set::run_image(&client, service, image).await?;
                }
                SetCommand::Env { service, vars } => {
                    set::run_env(&client, service, vars, false).await?;
                }
                SetCommand::Replicas { service, replicas } => {
                    set::run_replicas(&client, service, replicas).await?;
                }
            },
            Commands::Drain { name } => {
                cordon::run_drain(&client, name).await?;
            }
            Commands::Edit { resource, name } => {
                let resource_str = resource.to_string();
                edit::run(&client, &resource_str, name).await?;
            }
            Commands::Diff {
                resource,
                name,
                filename,
            } => {
                let resource_str = resource.to_string();
                diff::run(&client, &resource_str, name, filename).await?;
            }
            Commands::Patch {
                resource,
                name,
                patch,
                patch_type: _,
                dry_run,
            } => {
                let resource_str = resource.to_string();
                let patch_content = if let Some(stripped) = patch.strip_prefix('@') {
                    std::fs::read_to_string(stripped)?
                } else {
                    patch
                };
                patch::run(&client, &resource_str, name, patch_content, dry_run).await?;
            }
            Commands::Cordon { name } => {
                cordon::run_cordon(&client, name).await?;
            }
            Commands::Uncordon { name } => {
                cordon::run_uncordon(&client, name).await?;
            }
            Commands::Explain {
                resource,
                recursive,
                api_version,
            } => {
                let resource_str = resource.map(|r| r.to_string());
                explain::run(resource_str).await?;
            }
            Commands::Wait {
                resource,
                name,
                condition,
                timeout,
                jsonpath,
            } => {
                let resource_str = resource.to_string();
                wait::run(&client, &resource_str, name, condition, timeout).await?;
            }
            Commands::Stack { command } => match command {
                StackCommand::Deploy { compose_file, name } => {
                    stack::deploy(&client, compose_file, name).await?;
                }
                StackCommand::Rm { name } => {
                    stack::remove(&client, &name).await?;
                }
                StackCommand::Ls => {
                    stack::list(&client).await?;
                }
            },
            Commands::Config { command } => match command {
                ConfigCommand::Ls => {
                    config::run_ls(&client, cli.output).await?;
                }
                ConfigCommand::Create {
                    name,
                    from_file,
                    stdin,
                } => {
                    config::run_create(&client, name, from_file, stdin).await?;
                }
                ConfigCommand::Rm { name } => {
                    config::run_rm(&client, name).await?;
                }
                ConfigCommand::Inspect { name } => {
                    config::run_inspect(&client, name, cli.output).await?;
                }
                ConfigCommand::View { name } => {
                    config::run_view(&client, name).await?;
                }
            },
            Commands::ApiResources => {
                api_resources::run().await?;
            }
            Commands::Taint {
                name,
                taints,
                remove,
                overwrite,
            } => {
                taint::run(&client, name, taints, remove, overwrite).await?;
            }
            Commands::Replace {
                resource,
                filename,
                stdin,
                dry_run,
                force,
            } => {
                replace::run(&client, resource, filename, stdin, dry_run, force).await?;
            }
            Commands::Swarm { command } => match command {
                SwarmCommand::Init { advertise_addr } => {
                    swarm::run_init(&client, advertise_addr).await?;
                }
                SwarmCommand::Join {
                    token,
                    remote,
                    advertise_addr,
                } => {
                    swarm::run_join(&client, token, remote, advertise_addr).await?;
                }
                SwarmCommand::Leave { force } => {
                    swarm::run_leave(&client, force).await?;
                }
                SwarmCommand::Token {
                    worker,
                    manager,
                    rotate,
                } => {
                    swarm::run_token(&client, worker, manager, rotate).await?;
                }
                SwarmCommand::Status => {
                    swarm::run_status(&client).await?;
                }
            },
            Commands::Vault { command } => match command {
                VaultCommand::Init => {
                    vault::run_init().await?;
                }
                VaultCommand::Status => {
                    vault::run_status().await?;
                }
                VaultCommand::Unlock => {
                    vault::run_unlock().await?;
                }
                VaultCommand::SetKey => {
                    vault::run_set_key().await?;
                }
            },
            Commands::Discover {
                subnet,
                json,
                no_tui,
            } => {
                if json || no_tui {
                    discover::run_scan(&client, subnet.clone(), json).await?;
                } else {
                    #[cfg(feature = "tui")]
                    {
                        tui::discover::run_discovery_tui(&client, subnet.clone()).await?;
                    }
                    #[cfg(not(feature = "tui"))]
                    {
                        discover::run_interactive(&client, subnet.clone()).await?;
                    }
                }
            },
            Commands::Promote { name } => {
                let node_id = crate::api::node::get_node_id_by_hostname(client.inner(), &name).await?;
                let node_id = node_id.ok_or_else(|| anyhow::anyhow!("Node '{}' not found", name))?;
                crate::api::node::promote_node(client.inner(), &node_id).await?;
                println!("Node {} promoted to manager.", name);
            }
            Commands::Demote { name } => {
                let node_id = crate::api::node::get_node_id_by_hostname(client.inner(), &name).await?;
                let node_id = node_id.ok_or_else(|| anyhow::anyhow!("Node '{}' not found", name))?;
                crate::api::node::demote_node(client.inner(), &node_id).await?;
                println!("Node {} demoted to worker.", name);
            }
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
                | Commands::Dashboard
                | Commands::Apply { .. }
                | Commands::Run { .. }
                | Commands::Events
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
        )
    }

    fn detect_local_ip() -> String {
        use std::net::UdpSocket;
        let socket = UdpSocket::bind("0.0.0.0:0").ok();
        socket
            .and_then(|s| {
                s.connect("8.8.8.8:80").ok()?;
                s.local_addr().ok()
            })
            .map(|addr| addr.ip().to_string())
            .unwrap_or_else(|| "127.0.0.1".to_string())
    }
}
