use bollard::Docker;

#[allow(dead_code)]
pub async fn get_cluster_info(_docker: &Docker) -> anyhow::Result<bollard::models::ClusterInfo> {
    Err(anyhow::anyhow!("Not implemented"))
}

#[allow(dead_code)]
pub async fn get_swarm_info(_docker: &Docker) -> anyhow::Result<bollard::models::Swarm> {
    Err(anyhow::anyhow!("Not implemented"))
}
