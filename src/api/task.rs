use bollard::Docker;

pub async fn list_tasks(docker: &Docker) -> anyhow::Result<Vec<bollard::models::Task>> {
    Ok(docker
        .list_tasks(None::<bollard::query_parameters::ListTasksOptions>)
        .await?)
}
