use bollard::Docker;

#[allow(dead_code)]
pub async fn list_configs(_docker: &Docker) -> anyhow::Result<Vec<bollard::models::Config>> {
    Ok(Vec::new())
}

#[allow(dead_code)]
pub async fn inspect_config(_docker: &Docker, _id: &str) -> anyhow::Result<bollard::models::Config> {
    Err(anyhow::anyhow!("Not implemented"))
}
