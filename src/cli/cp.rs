use crate::api::client::DockerClient;
use bollard::query_parameters::{DownloadFromContainerOptions, UploadToContainerOptions};
use futures::StreamExt;

pub async fn run(client: &DockerClient, source: String, target: String) -> anyhow::Result<()> {
    let (from_service, from_path, to_service, local_target) = if source.contains(':')
        && !target.contains(':')
    {
        let parts: Vec<&str> = source.splitn(2, ':').collect();
        (
            Some(parts[0].to_string()),
            parts[1].to_string(),
            None,
            target,
        )
    } else if target.contains(':') && !source.contains(':') {
        let parts: Vec<&str> = target.splitn(2, ':').collect();
        (
            None,
            String::new(),
            Some(parts[0].to_string()),
            parts[1].to_string(),
        )
    } else {
        return Err(anyhow::anyhow!(
            "Usage: swarmctl cp <service>:<path> <local-path> or swarmctl cp <local-path> <service>:<path>"
        ));
    };

    if let Some(service_name) = from_service {
        copy_from_container(client, &service_name, &from_path, &local_target).await?;
    } else if let Some(service_name) = to_service {
        copy_to_container(client, &source, &service_name, &local_target).await?;
    }

    Ok(())
}

async fn copy_from_container(
    client: &DockerClient,
    service_name: &str,
    container_path: &str,
    local_path: &str,
) -> anyhow::Result<()> {
    let (_task_id, container_id) =
        crate::api::task::running_container_for_service(client, service_name).await?;

    let options = DownloadFromContainerOptions {
        path: container_path.to_string(),
    };

    let mut stream = client
        .inner()
        .download_from_container(&container_id, Some(options));
    let mut data = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(chunk) => data.extend_from_slice(&chunk),
            Err(e) => return Err(anyhow::anyhow!("Error downloading: {}", e)),
        }
    }

    let path = std::path::Path::new(local_path);
    let filename = std::path::Path::new(container_path)
        .file_name()
        .unwrap_or_default();

    if path.is_dir() || local_path.ends_with('/') {
        std::fs::write(path.join(filename), &data)?;
        println!(
            "Copied {} to {}/{}",
            container_path,
            local_path,
            filename.to_string_lossy()
        );
    } else {
        std::fs::write(local_path, &data)?;
        println!("Copied {} to {}", container_path, local_path);
    }

    Ok(())
}

async fn copy_to_container(
    client: &DockerClient,
    local_path: &str,
    service_name: &str,
    container_path: &str,
) -> anyhow::Result<()> {
    let (_task_id, container_id) =
        crate::api::task::running_container_for_service(client, service_name).await?;

    let data = std::fs::read(local_path)?;
    let options = UploadToContainerOptions {
        path: container_path.to_string(),
        ..Default::default()
    };

    client
        .inner()
        .upload_to_container(
            &container_id,
            Some(options),
            bollard::body_full(data.into()),
        )
        .await?;
    println!(
        "Copied {} to {}:{}",
        local_path, service_name, container_path
    );

    Ok(())
}
