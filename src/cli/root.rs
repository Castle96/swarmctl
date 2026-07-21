use crate::api::client::DockerClient;
use crate::cli::{
    apply, attach, cluster, completion, config, cordon, cp, create, delete, describe, diff, edit,
    events, exec, explain, get, label, logs, patch, port_forward, ports, rollout, run, scale, set,
    stack, top, wait,
};
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

    /// Path to TLS CA certificate
    #[arg(long)]
    tlscacert: Option<String>,

    /// Path to TLS client certificate
    #[arg(long)]
    tlscert: Option<String>,

    /// Path to TLS client key
    #[arg(long)]
    tlskey: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
}

#[derive(Subcommand)]
enum Commands {
    /// Display one or many resources
    Get {
        /// Resource type
        resource: ResourceType,

        /// Resource name
        name: Option<String>,

        /// Show labels
        #[arg(long)]
        show_labels: bool,

        /// Filter by label selector
        #[arg(long)]
        selector: Option<String>,
    },

    /// Show detailed information about a resource
    Describe {
        /// Resource type
        resource: ResourceType,

        /// Resource name
        name: String,
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
    },

    /// List and visualize port mappings on the swarm
    Ports {
        /// Enable TUI visualization mode
        #[arg(short, long)]
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
    Dashboard,

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
        name: String,

        /// Labels to set (KEY=VAL)
        labels: Vec<String>,

        /// Overwrite existing labels (set only these)
        #[arg(long)]
        overwrite: bool,

        /// Delete all labels on the resource
        #[arg(long)]
        all: bool,
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
        /// Resource type (service, node)
        resource: ResourceType,

        /// Resource name
        name: String,

        /// JSON patch content (inline or file with @prefix)
        #[arg(short, long)]
        patch: String,
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

        let conn_config = crate::api::client::ConnectionConfig {
            host: cli.host.clone(),
            tlscacert: cli.tlscacert.clone(),
            tlscert: cli.tlscert.clone(),
            tlskey: cli.tlskey.clone(),
        };
        let client = DockerClient::with_config(&conn_config)?;

        match cli.command {
            Commands::Get {
                resource,
                name,
                show_labels,
                selector,
            } => {
                get::run(
                    &client,
                    resource,
                    name,
                    cli.output,
                    show_labels,
                    selector,
                    cli.watch,
                )
                .await?;
            }
            Commands::Describe { resource, name } => {
                describe::run(&client, resource, name, cli.output).await?;
            }
            Commands::Create {
                resource,
                name,
                filename,
                stdin,
            } => {
                create::run(&client, resource, name, filename, stdin).await?;
            }
            Commands::Delete {
                resource,
                name,
                selector,
                force,
            } => {
                delete::run(&client, resource, name, selector, force).await?;
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
            } => {
                logs::run(&client, resource, name, follow, tail, previous).await?;
            }
            Commands::Ports {
                tui,
                available,
                range_start,
                range_end,
                protocol,
            } => {
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
            }
            Commands::ClusterInfo => {
                cluster::run(&client).await?;
            }
            Commands::Dashboard => {
                crate::tui::run_tui(&client).await?;
            }
            Commands::Apply {
                resource,
                name,
                filename,
                stdin,
            } => {
                apply::run(&client, resource, name, filename, stdin).await?;
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
            },
            Commands::Version => {
                println!("swarmctl version 0.1.0");
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
            } => {
                let resource_str = resource.to_string();
                label::run(&client, &resource_str, name, labels, overwrite, all).await?;
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
            } => {
                let resource_str = resource.to_string();
                let patch_content = if let Some(stripped) = patch.strip_prefix('@') {
                    std::fs::read_to_string(stripped)?
                } else {
                    patch
                };
                patch::run(&client, &resource_str, name, patch_content).await?;
            }
            Commands::Cordon { name } => {
                cordon::run_cordon(&client, name).await?;
            }
            Commands::Uncordon { name } => {
                cordon::run_uncordon(&client, name).await?;
            }
            Commands::Explain { resource } => {
                let resource_str = resource.map(|r| r.to_string());
                explain::run(resource_str).await?;
            }
            Commands::Wait {
                resource,
                name,
                condition,
                timeout,
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
        }

        Ok(())
    }
}
