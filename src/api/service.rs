use bollard::Docker;
use bollard::models::Service;

pub async fn list_services(docker: &Docker) -> anyhow::Result<Vec<Service>> {
    Ok(docker
        .list_services(None::<bollard::query_parameters::ListServicesOptions>)
        .await?)
}

/// A service's name, which lives under `Spec`.
pub fn service_name(service: &Service) -> Option<&str> {
    service.spec.as_ref().and_then(|s| s.name.as_deref())
}

/// Find a service by name.
///
/// The "list everything, then linear scan for a matching `Spec.Name`" block was
/// repeated in 33 places; each also allocated a fresh `String` per element
/// inside the `find` closure.
pub async fn find_service(docker: &Docker, name: &str) -> anyhow::Result<Service> {
    list_services(docker)
        .await?
        .into_iter()
        .find(|s| service_name(s) == Some(name))
        .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", name))
}

/// Find a service by name, or `None`.
pub async fn find_service_opt(docker: &Docker, name: &str) -> anyhow::Result<Option<Service>> {
    Ok(list_services(docker)
        .await?
        .into_iter()
        .find(|s| service_name(s) == Some(name)))
}
