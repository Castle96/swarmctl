use bollard::Docker;

pub async fn list_secrets(docker: &Docker) -> anyhow::Result<Vec<bollard::models::Secret>> {
    Ok(docker
        .list_secrets(None::<bollard::query_parameters::ListSecretsOptions>)
        .await?)
}

/// Find a secret by name.
pub async fn find_secret(docker: &Docker, name: &str) -> anyhow::Result<bollard::models::Secret> {
    list_secrets(docker)
        .await?
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_deref()) == Some(name))
        .ok_or_else(|| anyhow::anyhow!("Secret '{}' not found", name))
}

/// Find a secret by name, or `None`.
pub async fn find_secret_opt(
    docker: &Docker,
    name: &str,
) -> anyhow::Result<Option<bollard::models::Secret>> {
    Ok(list_secrets(docker)
        .await?
        .into_iter()
        .find(|x| x.spec.as_ref().and_then(|sp| sp.name.as_deref()) == Some(name)))
}
