use crate::cli::explain::SHORT_NAMES;
use anyhow::Result;

pub async fn run() -> Result<()> {
    // Report the API version this binary actually speaks. The table used to
    // claim "v1.25+" for every resource, which is fiction: swarmctl negotiates
    // bollard's API_DEFAULT_VERSION.
    let api_version = format!("v{}", bollard::API_DEFAULT_VERSION);

    println!(
        "{:<20} {:<12} {:<12} {:<12}",
        "NAME", "SHORTNAMES", "APIVERSION", "NAMESPACED"
    );
    println!("{}", "-".repeat(58));
    println!(
        "{:<20} {:<12} {:<12} {:<12}",
        "*swarmctl", "swarmctl", api_version, "false"
    );
    println!("{}", "-".repeat(58));

    // Sourced from `explain`, which is tested against the parser's ResourceType,
    // so neither table can drift from the CLI's actual resource list.
    for (name, short) in SHORT_NAMES {
        println!(
            "{:<20} {:<12} {:<12} {:<12}",
            name,
            short,
            api_version,
            // Docker Swarm has no namespaces; stacks group by a label instead.
            "false"
        );
    }

    println!();
    println!("Use 'swarmctl explain <resource>' for field details.");
    println!("Note: Swarm has no namespaces. Stack grouping uses the");
    println!("'com.docker.stack.namespace' label.");

    Ok(())
}
