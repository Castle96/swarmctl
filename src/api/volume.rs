use bollard::Docker;
use bollard::models::Volume;
use bollard::query_parameters::{ListVolumesOptions, RemoveVolumeOptions};
use bollard::models::VolumeCreateRequest;
use std::collections::HashMap;

pub async fn list_volumes(docker: &Docker) -> anyhow::Result<Vec<Volume>> {
    let options = ListVolumesOptions {
        ..Default::default()
    };
    let result = docker.list_volumes(Some(options)).await?;
    Ok(result.volumes.unwrap_or_default())
}

pub async fn inspect_volume(docker: &Docker, name: &str) -> anyhow::Result<Volume> {
    Ok(docker.inspect_volume(name).await?)
}

pub async fn remove_volume(docker: &Docker, name: &str, force: bool) -> anyhow::Result<()> {
    let options = RemoveVolumeOptions {
        force,
    };
    docker.remove_volume(name, Some(options)).await?;
    Ok(())
}

pub async fn create_volume(
    docker: &Docker,
    name: &str,
    driver: &str,
    labels: HashMap<String, String>,
) -> anyhow::Result<Volume> {
    let request = VolumeCreateRequest {
        name: Some(name.to_string()),
        driver: Some(driver.to_string()),
        labels: Some(labels),
        ..Default::default()
    };
    Ok(docker.create_volume(request).await?)
}

#[allow(dead_code)]
pub async fn get_volume_mountpoints(docker: &Docker, volume_name: &str) -> anyhow::Result<Vec<String>> {
    let options = bollard::query_parameters::ListContainersOptions {
        all: true,
        ..Default::default()
    };
    let containers = docker.list_containers(Some(options)).await?;
    let mut mountpoints = Vec::new();

    for container in containers {
        if let Some(mounts) = &container.mounts {
            for mount in mounts {
                if mount.name.as_deref() == Some(volume_name) {
                    if let Some(source) = &mount.source {
                        mountpoints.push(source.clone());
                    }
                }
            }
        }
    }

    Ok(mountpoints)
}
