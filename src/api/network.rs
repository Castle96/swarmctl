use bollard::Docker;

pub async fn list_networks(docker: &Docker) -> anyhow::Result<Vec<bollard::models::Network>> {
    Ok(docker
        .list_networks(None::<bollard::query_parameters::ListNetworksOptions>)
        .await?)
}

/// Find a network by name or id.
pub async fn find_network(docker: &Docker, name: &str) -> anyhow::Result<bollard::models::Network> {
    list_networks(docker)
        .await?
        .into_iter()
        .find(|n| n.name.as_deref() == Some(name) || n.id.as_deref() == Some(name))
        .ok_or_else(|| anyhow::anyhow!("Network '{}' not found", name))
}
