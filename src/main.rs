use cli::root::Cli;

mod api;
mod cli;
mod models;
#[cfg(feature = "tui")]
mod tui;
mod utils;
mod vault;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    Cli::run().await
}
