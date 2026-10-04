use crate::api::client::DockerClient;

pub async fn run(client: &DockerClient, name: &str, replicas: u64) -> anyhow::Result<()> {
    println!("Scaling service {} to {} replicas...", name, replicas);

    // Find the service
    let service = crate::api::service::find_service(client.inner(), name).await?;

    // Update the service spec with new replica count
    let mut spec = service.spec.unwrap_or_default();

    if let Some(mode) = &mut spec.mode {
        if let Some(replicated) = &mut mode.replicated {
            replicated.replicas = Some(replicas as i64);
        } else {
            return Err(anyhow::anyhow!(
                "Service {} is not in replicated mode",
                name
            ));
        }
    } else {
        return Err(anyhow::anyhow!("Service {} has no mode specified", name));
    }

    // Update the service
    let options = bollard::query_parameters::UpdateServiceOptions {
        version: service.version.unwrap().index.unwrap() as i32,
        ..Default::default()
    };

    client
        .inner()
        .update_service(name, spec, options, None)
        .await?;

    println!("Service {} scaled to {} replicas", name, replicas);

    Ok(())
}
