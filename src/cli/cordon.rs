use crate::api::client::DockerClient;
use bollard::models::NodeSpecAvailabilityEnum;

pub async fn run_cordon(client: &DockerClient, name: String) -> anyhow::Result<()> {
    set_availability(client, &name, NodeSpecAvailabilityEnum::PAUSE, "cordoned").await
}

pub async fn run_uncordon(client: &DockerClient, name: String) -> anyhow::Result<()> {
    set_availability(
        client,
        &name,
        NodeSpecAvailabilityEnum::ACTIVE,
        "uncordoned",
    )
    .await
}

pub async fn run_drain(client: &DockerClient, name: String) -> anyhow::Result<()> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;
    let node = nodes
        .into_iter()
        .find(|n| n.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found", name))?;

    let version = node.version.and_then(|v| v.index).unwrap_or(0) as i64;
    let mut spec = node.spec.unwrap_or_default();
    spec.availability = Some(NodeSpecAvailabilityEnum::DRAIN);

    let opts = bollard::query_parameters::UpdateNodeOptionsBuilder::default()
        .version(version)
        .build();
    client.inner().update_node(&name, spec, opts).await?;
    println!(
        "Node '{}' drained (tasks will be rescheduled by Swarm)",
        name
    );
    Ok(())
}

async fn set_availability(
    client: &DockerClient,
    name: &str,
    availability: NodeSpecAvailabilityEnum,
    action: &str,
) -> anyhow::Result<()> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;
    let node = nodes
        .into_iter()
        .find(|n| n.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found", name))?;

    let version = node.version.and_then(|v| v.index).unwrap_or(0) as i64;
    let mut spec = node.spec.unwrap_or_default();
    spec.availability = Some(availability);

    let opts = bollard::query_parameters::UpdateNodeOptionsBuilder::default()
        .version(version)
        .build();
    client.inner().update_node(name, spec, opts).await?;
    println!("Node '{}' {}", name, action);
    Ok(())
}
