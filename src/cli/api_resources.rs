use crate::api::client::DockerClient;
use crate::cli::kobject;
use crate::cli::registry;
use anyhow::Result;

/// Print the resource table.
///
/// This used to print `bollard::API_DEFAULT_VERSION` unconditionally, with no
/// Docker client at all, plus a fabricated `*swarmctl` root row that no command
/// could address. It now connects and reports which daemon it reached.
///
/// The APIVERSION column is deliberately the version **swarmctl speaks**, not
/// the daemon's release number. Those are different things — Docker 29.9.0
/// serves API 1.56 while swarmctl negotiates v1.52 — and the API version is what
/// a `.VERSION` resource qualifier is checked against, so it is the one that has
/// to be right.
pub async fn run(client: &DockerClient) -> Result<()> {
    match client.inner().info().await {
        Ok(info) => print_table(
            &kobject::api_version(),
            info.server_version.as_deref(),
            info.name.as_deref(),
        ),
        Err(e) => {
            log::debug!("api-resources could not read /info: {e}");
            print_table(&kobject::api_version(), None, None)
        }
    }
}

/// Print the table without a daemon, using the compiled-in API version.
pub async fn run_offline() -> Result<()> {
    print_table(&kobject::api_version(), None, None)
}

const SEPARATOR: &str = "----------------------------------------------------------------";

fn print_table(
    api_version: &str,
    daemon_version: Option<&str>,
    daemon_name: Option<&str>,
) -> Result<()> {
    println!(
        "{:<16} {:<14} {:<12} {:<12} KIND",
        "NAME", "SHORTNAMES", "APIVERSION", "NAMESPACED"
    );
    println!("{SEPARATOR}");

    for info in registry::REGISTRY {
        println!(
            "{:<16} {:<14} {:<12} {:<12} {}",
            info.plural,
            registry::short_names_joined(info),
            api_version,
            info.namespaced,
            info.kind
        );
    }

    println!();
    match (daemon_version, daemon_name) {
        (Some(v), Some(n)) => println!("Daemon: Docker {v} on {n}."),
        (Some(v), None) => println!("Daemon: Docker {v}."),
        _ => {
            println!("Could not reach the daemon; showing the API version swarmctl was built for.")
        }
    }
    println!("APIVERSION is the Engine API version swarmctl speaks.");
    println!("Swarm has no namespaces and no API groups. Stack grouping uses the");
    println!("'com.docker.stack.namespace' label.");
    println!("Use 'swarmctl explain <resource>' for field details.");

    Ok(())
}
