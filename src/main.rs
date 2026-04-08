use cli::root::Cli;

mod api;
mod cli;
mod models;
mod tui;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    Cli::run().await
}