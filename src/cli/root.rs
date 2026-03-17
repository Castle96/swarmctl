use clap::{Parser, Subcommand};
use crate::cli::{node, service};
use crate::api::client::DockerClient;

#[derive(Parser)]
#[command(name = "swarmctl")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Node {
        #[command(subcommand)]
        command: NodeCommand,
    },
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
}

#[derive(Subcommand)]
enum NodeCommand {
    List,
}

#[derive(Subcommand)]
enum ServiceCommand {
    List,
}

impl Cli {
    pub async fn run() -> anyhow::Result<()> {
        let cli = Cli::parse();
        let client = DockerClient::new();

        match cli.command {
            Commands::Node { command } => match command {
                NodeCommand::List => node::list(&client).await?,
            },
            Commands::Service { command } => match command {
                ServiceCommand::List => service::list(&client).await?,
            },
        }

        Ok(())
    }
}