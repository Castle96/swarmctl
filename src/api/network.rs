use bollard::Docker;

pub async fn list_networks(docker: &Docker) -> anyhow::Result<Vec<bollard::models::Network>> {
    Ok(docker
        .list_networks(None::<bollard::query_parameters::ListNetworksOptions>)
        .await?)
}

#[allow(dead_code)]
pub async fn inspect_network(_docker: &Docker, _id: &str) -> anyhow::Result<bollard::models::Network> {
    Err(anyhow::anyhow!("Not implemented"))
}
