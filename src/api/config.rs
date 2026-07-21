use bollard::Docker;

pub async fn list_configs(docker: &Docker) -> anyhow::Result<Vec<bollard::models::Config>> {
    Ok(docker
        .list_configs(None::<bollard::query_parameters::ListConfigsOptions>)
        .await?)
}
