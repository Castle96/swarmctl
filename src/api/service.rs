use bollard::Docker;
use bollard::models::Service;
use bollard::query_parameters::ListServicesOptions;

pub async fn list_services(docker: &Docker) -> anyhow::Result<Vec<Service>> {
    Ok(docker
        .list_services(None::<ListServicesOptions<String>>)
        .await?)
}