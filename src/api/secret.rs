use bollard::Docker;

#[allow(dead_code)]
pub async fn list_secrets(_docker: &Docker) -> anyhow::Result<Vec<bollard::models::Secret>> {
    Ok(Vec::new())
}

#[allow(dead_code)]
pub async fn inspect_secret(_docker: &Docker, _id: &str) -> anyhow::Result<bollard::models::Secret> {
    Err(anyhow::anyhow!("Not implemented"))
}
