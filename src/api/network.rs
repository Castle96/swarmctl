use bollard::Docker;

#[allow(dead_code)]
pub async fn list_networks(_docker: &Docker) -> anyhow::Result<Vec<bollard::models::Network>> {
    Ok(Vec::new())
}

#[allow(dead_code)]
pub async fn inspect_network(_docker: &Docker, _id: &str) -> anyhow::Result<bollard::models::Network> {
    Err(anyhow::anyhow!("Not implemented"))
}
