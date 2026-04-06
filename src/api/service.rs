use bollard::Docker;
use bollard::models::Service;

pub async fn list_services(docker: &Docker) -> anyhow::Result<Vec<Service>> {
    Ok(docker
        .list_services(None::<bollard::query_parameters::ListServicesOptions>)
        .await?)
}