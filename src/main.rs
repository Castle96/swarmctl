use cli::root::Cli;

mod api;
mod cli;
mod models;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    Cli::run().await
}