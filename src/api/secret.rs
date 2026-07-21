use bollard::Docker;

pub async fn list_secrets(docker: &Docker) -> anyhow::Result<Vec<bollard::models::Secret>> {
    Ok(docker
        .list_secrets(None::<bollard::query_parameters::ListSecretsOptions>)
        .await?)
}
