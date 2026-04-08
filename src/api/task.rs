use bollard::Docker;

#[allow(dead_code)]
pub async fn list_tasks(_docker: &Docker) -> anyhow::Result<Vec<bollard::models::Task>> {
    Ok(Vec::new())
}

#[allow(dead_code)]
pub async fn inspect_task(_docker: &Docker, _id: &str) -> anyhow::Result<bollard::models::Task> {
    Err(anyhow::anyhow!("Not implemented"))
}
