use bollard::Docker;

pub async fn list_secrets(docker: &Docker) -> anyhow::Result<Vec<bollard::models::Secret>> {
    Ok(docker
        .list_secrets(None::<bollard::query_parameters::ListSecretsOptions>)
        .await?)
}

#[allow(dead_code)]
pub async fn inspect_secret(_docker: &Docker, _id: &str) -> anyhow::Result<bollard::models::Secret> {
    Err(anyhow::anyhow!("Not implemented"))
}
