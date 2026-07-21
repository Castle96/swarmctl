use crate::api::client::DockerClient;
use std::collections::HashMap;

pub async fn run_image(
    client: &DockerClient,
    service_name: String,
    image: String,
) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let service = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&service_name))
        .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", service_name))?;

    let version = service.version.and_then(|v| v.index).unwrap_or(0) as i32;
    let mut spec = service.spec.unwrap_or_default();

    if let Some(ref mut task_template) = spec.task_template {
        if let Some(ref mut container_spec) = task_template.container_spec {
            container_spec.image = Some(image.clone());
        } else {
            task_template.container_spec = Some(bollard::models::TaskSpecContainerSpec {
                image: Some(image.clone()),
                ..Default::default()
            });
        }
    } else {
        spec.task_template = Some(bollard::models::TaskSpec {
            container_spec: Some(bollard::models::TaskSpecContainerSpec {
                image: Some(image.clone()),
                ..Default::default()
            }),
            ..Default::default()
        });
    }

    let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
        .version(version)
        .build();
    client
        .inner()
        .update_service(&service_name, spec, opts, None)
        .await?;
    println!("Service '{}' image updated to '{}'", service_name, image);
    Ok(())
}

pub async fn run_env(
    client: &DockerClient,
    service_name: String,
    env: Vec<String>,
    replace: bool,
) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let service = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&service_name))
        .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", service_name))?;

    let version = service.version.and_then(|v| v.index).unwrap_or(0) as i32;
    let mut spec = service.spec.unwrap_or_default();

    let new_vars: Vec<String> = if replace {
        env.clone()
    } else {
        let existing: Vec<String> = spec
            .task_template
            .as_ref()
            .and_then(|t| t.container_spec.as_ref())
            .and_then(|c| c.env.clone())
            .unwrap_or_default();

        let mut map: HashMap<String, String> = HashMap::new();
        for e in &existing {
            if let Some((k, v)) = e.split_once('=') {
                map.insert(k.to_string(), v.to_string());
            }
        }
        for e in &env {
            if let Some((k, v)) = e.split_once('=') {
                map.insert(k.to_string(), v.to_string());
            } else {
                map.insert(e.clone(), String::new());
            }
        }
        map.into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect()
    };

    if let Some(ref mut task_template) = spec.task_template
        && let Some(ref mut container_spec) = task_template.container_spec {
            container_spec.env = Some(new_vars);
        }

    let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
        .version(version)
        .build();
    client
        .inner()
        .update_service(&service_name, spec, opts, None)
        .await?;
    println!("Service '{}' environment updated", service_name);
    Ok(())
}

pub async fn run_replicas(
    client: &DockerClient,
    service_name: String,
    replicas: u64,
) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let service = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&service_name))
        .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", service_name))?;

    let version = service.version.and_then(|v| v.index).unwrap_or(0) as i32;
    let mut spec = service.spec.unwrap_or_default();

    if let Some(ref mut mode) = spec.mode {
        if let Some(ref mut replicated) = mode.replicated {
            replicated.replicas = Some(replicas as i64);
        } else {
            mode.replicated = Some(bollard::models::ServiceSpecModeReplicated {
                replicas: Some(replicas as i64),
            });
        }
    } else {
        spec.mode = Some(bollard::models::ServiceSpecMode {
            replicated: Some(bollard::models::ServiceSpecModeReplicated {
                replicas: Some(replicas as i64),
            }),
            ..Default::default()
        });
    }

    let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
        .version(version)
        .build();
    client
        .inner()
        .update_service(&service_name, spec, opts, None)
        .await?;
    println!("Service '{}' replicas set to {}", service_name, replicas);
    Ok(())
}
