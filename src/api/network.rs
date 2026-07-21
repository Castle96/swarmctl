use bollard::Docker;

pub async fn list_networks(docker: &Docker) -> anyhow::Result<Vec<bollard::models::Network>> {
    Ok(docker
        .list_networks(None::<bollard::query_parameters::ListNetworksOptions>)
        .await?)
}
