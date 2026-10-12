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
    // `-v` is repeatable: 0 warnings only, 1 info (default), 2 debug, 3+ trace.
    // Previously the flag was parsed and read nowhere, so `swarmctl -vv` did
    // nothing, and env_logger's `error` default hid the warnings that make
    // endpoint and context problems diagnosable.
    let verbose = Cli::verbose_count(std::env::args().skip(1));
    env_logger::Builder::new()
        .filter_level(match verbose {
            0 => log::LevelFilter::Warn,
            1 => log::LevelFilter::Info,
            2 => log::LevelFilter::Debug,
            _ => log::LevelFilter::Trace,
        })
        .parse_default_env()
        .init();

    Cli::run().await
}

// deliberate break
fn broken() { let _: i32 = "not an int"; }
