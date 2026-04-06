use clap::{Parser, Subcommand, ValueEnum};
use crate::cli::{get, describe, create, delete, logs, scale};
use crate::api::client::DockerClient;

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
    },

    /// Get cluster info
    ClusterInfo,

    /// Show version information
    Version,
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

#[derive(Clone, ValueEnum)]
pub enum LogResourceType {
    Service,
    Task,
}

impl Cli {
    pub async fn run() -> anyhow::Result<()> {
        let cli = Cli::parse();
        let client = DockerClient::new();

        match cli.command {
            Commands::Get { resource, name, show_labels, selector } => {
                get::run(&client, resource, name, cli.output, show_labels, selector, cli.watch).await?;
            }
            Commands::Describe { resource, name } => {
                describe::run(&client, resource, name, cli.output).await?;
            }
            Commands::Create { resource, name, filename, stdin } => {
                create::run(&client, resource, name, filename, stdin).await?;
            }
            Commands::Delete { resource, name, selector, force } => {
                delete::run(&client, resource, name, selector, force).await?;
            }
            Commands::Scale { name, replicas } => {
                scale::run(&client, &name, replicas).await?;
            }
            Commands::Logs { resource, name, follow, tail } => {
                logs::run(&client, resource, name, follow, tail).await?;
            }
            Commands::ClusterInfo => {
                println!("Swarm cluster information:");
                // TODO: Implement cluster info
            }
            Commands::Version => {
                println!("swarmctl version 0.1.0");
                println!("Docker API version: {}", bollard::API_DEFAULT_VERSION);
            }
        }

        Ok(())
    }
}