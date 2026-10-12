//! `diff <resource> <name> -f <file>`: compare a live resource against a file.
//!
//! This used to shell out to the system `diff -u` on fully serialized Bollard
//! objects. Live objects carry server-assigned noise (`Version`, `CreatedAt`,
//! `UpdateStatus`, node bookkeeping) that no manifest can reproduce, so every
//! run printed a wall of false positives that buried the real change. The
//! structured comparison reports only the fields a manifest can declare, so
//! one printed line means one real difference.

use crate::api::client::DockerClient;
use crate::api::specdiff::compare_objects;
use crate::cli::root::ResourceType;

/// Kinds `diff` can compare.
fn supported(resource: ResourceType) -> anyhow::Result<()> {
    match resource {
        ResourceType::Services
        | ResourceType::Nodes
        | ResourceType::Secrets
        | ResourceType::Configs => Ok(()),
        other => anyhow::bail!(
            "diff is not supported for {}. Diffable kinds: services, nodes, \
             secrets, configs.",
            other
        ),
    }
}

/// The live object for a resource, as raw JSON.
async fn live_object(
    client: &DockerClient,
    resource: ResourceType,
    name: &str,
) -> anyhow::Result<serde_json::Value> {
    let docker = client.inner();
    let object = match resource {
        ResourceType::Services => {
            serde_json::to_value(crate::api::service::find_service(docker, name).await?)?
        }
        ResourceType::Nodes => {
            serde_json::to_value(crate::api::node::find_node(docker, name).await?)?
        }
        ResourceType::Secrets => {
            serde_json::to_value(crate::api::secret::find_secret(docker, name).await?)?
        }
        ResourceType::Configs => {
            serde_json::to_value(crate::api::config::find_config(docker, name).await?)?
        }
        // Rejected by `supported` above.
        _ => unreachable!("diff dispatch reached for {resource:?}"),
    };
    Ok(object)
}

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: String,
    filename: String,
) -> anyhow::Result<()> {
    supported(resource)?;
    let live = live_object(client, resource, &name).await?;

    let content = std::fs::read_to_string(&filename)?;
    let desired: serde_json::Value = serde_json::from_str(&content).or_else(|_| {
        serde_yaml::from_str::<serde_json::Value>(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse {} as JSON or YAML: {}", filename, e))
    })?;

    let changes = compare_objects(&live, &desired);

    if changes.is_empty() {
        println!("No differences");
        return Ok(());
    }

    println!(
        "{} {} differs from {}:",
        resource_label(resource),
        name,
        filename
    );
    for change in &changes {
        println!("  {}: {} -> {}", change.path, change.live, change.desired);
    }

    // Same convention as the system tool this replaced: 0 when the inputs
    // match, 1 when they differ, so `diff ... && deploy` gates on the result.
    std::process::exit(1);
}

/// Lowercase singular label for a resource kind, for the header line.
fn resource_label(resource: ResourceType) -> &'static str {
    match resource {
        ResourceType::Services => "service",
        ResourceType::Nodes => "node",
        ResourceType::Secrets => "secret",
        ResourceType::Configs => "config",
        _ => "resource",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffable_kinds_are_accepted() {
        for resource in [
            ResourceType::Services,
            ResourceType::Nodes,
            ResourceType::Secrets,
            ResourceType::Configs,
        ] {
            assert!(supported(resource).is_ok(), "{resource:?} should diff");
        }
    }

    #[test]
    fn tasks_cannot_be_diffed() {
        let err = supported(ResourceType::Tasks).unwrap_err().to_string();
        assert!(err.contains("not supported"), "got: {err}");
    }

    #[test]
    fn resource_labels_are_lowercase_singular() {
        assert_eq!(resource_label(ResourceType::Services), "service");
        assert_eq!(resource_label(ResourceType::Nodes), "node");
        assert_eq!(resource_label(ResourceType::Secrets), "secret");
        assert_eq!(resource_label(ResourceType::Configs), "config");
    }
}
