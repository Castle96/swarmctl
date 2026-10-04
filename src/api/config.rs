use bollard::Docker;

pub async fn list_configs(docker: &Docker) -> anyhow::Result<Vec<bollard::models::Config>> {
    Ok(docker
        .list_configs(None::<bollard::query_parameters::ListConfigsOptions>)
        .await?)
}

/// Find a config by name.
pub async fn find_config(docker: &Docker, name: &str) -> anyhow::Result<bollard::models::Config> {
    list_configs(docker)
        .await?
        .into_iter()
        .find(|c| c.spec.as_ref().and_then(|sp| sp.name.as_deref()) == Some(name))
        .ok_or_else(|| anyhow::anyhow!("Config '{}' not found", name))
}

/// Find a config by name, or `None`.
pub async fn find_config_opt(
    docker: &Docker,
    name: &str,
) -> anyhow::Result<Option<bollard::models::Config>> {
    Ok(list_configs(docker)
        .await?
        .into_iter()
        .find(|x| x.spec.as_ref().and_then(|sp| sp.name.as_deref()) == Some(name)))
}
