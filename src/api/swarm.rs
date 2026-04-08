use bollard::Docker;

pub async fn get_cluster_info(docker: &Docker) -> anyhow::Result<bollard::models::Swarm> {
    Ok(docker.inspect_swarm().await?)
}

pub async fn get_swarm_info(docker: &Docker) -> anyhow::Result<bollard::models::Swarm> {
    Ok(docker.inspect_swarm().await?)
}
