use crate::api::client::DockerClient;
use std::collections::HashMap;

pub async fn run(
    client: &DockerClient,
    resource: &str,
    name: String,
    labels: Vec<String>,
    overwrite: bool,
    delete_all: bool,
) -> anyhow::Result<()> {
    let resource_type = resolve_resource_type(resource)?;

    match resource_type {
        "services" => update_service_labels(client, &name, labels, overwrite, delete_all).await?,
        "nodes" => update_node_labels(client, &name, labels, overwrite, delete_all).await?,
        "secrets" => update_secret_labels(client, &name, labels, overwrite, delete_all).await?,
        "configs" => update_config_labels(client, &name, labels, overwrite, delete_all).await?,
        _ => return Err(anyhow::anyhow!("label is not supported for {}", resource)),
    }

    Ok(())
}

fn resolve_resource_type(s: &str) -> anyhow::Result<&'static str> {
    match s.to_lowercase().as_str() {
        "services" | "svc" | "service" => Ok("services"),
        "nodes" | "no" | "node" => Ok("nodes"),
        "secrets" | "sec" | "secret" => Ok("secrets"),
        "configs" | "cm" | "config" => Ok("configs"),
        _ => Err(anyhow::anyhow!("Unknown resource type: {}", s)),
    }
}

fn parse_labels(labels: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for l in labels {
        if let Some((k, v)) = l.split_once('=') {
            map.insert(k.to_string(), v.to_string());
        } else {
            map.insert(l.clone(), String::new());
        }
    }
    map
}

async fn update_service_labels(
    client: &DockerClient,
    name: &str,
    labels: Vec<String>,
    overwrite: bool,
    delete_all: bool,
) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let service = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", name))?;

    let version = service.version.and_then(|v| v.index).unwrap_or(0) as i32;
    let mut spec = service.spec.unwrap_or_default();

    let new_labels = if delete_all {
        HashMap::new()
    } else if overwrite {
        parse_labels(&labels)
    } else {
        let mut existing = spec.labels.unwrap_or_default();
        for (k, v) in parse_labels(&labels) {
            existing.insert(k, v);
        }
        existing
    };

    spec.labels = if new_labels.is_empty() {
        None
    } else {
        Some(new_labels.clone())
    };

    let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
        .version(version)
        .build();
    client
        .inner()
        .update_service(name, spec, opts, None)
        .await?;

    let label_count = new_labels.len();
    println!("Service '{}' labels updated ({} labels)", name, label_count);
    Ok(())
}

async fn update_node_labels(
    client: &DockerClient,
    name: &str,
    labels: Vec<String>,
    overwrite: bool,
    delete_all: bool,
) -> anyhow::Result<()> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;
    let node = nodes
        .into_iter()
        .find(|n| n.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found", name))?;

    let version = node.version.and_then(|v| v.index).unwrap_or(0) as i64;
    let mut spec = node.spec.unwrap_or_default();

    let new_labels = if delete_all {
        HashMap::new()
    } else if overwrite {
        parse_labels(&labels)
    } else {
        let mut existing = spec.labels.unwrap_or_default();
        for (k, v) in parse_labels(&labels) {
            existing.insert(k, v);
        }
        existing
    };

    spec.labels = if new_labels.is_empty() {
        None
    } else {
        Some(new_labels.clone())
    };

    let opts = bollard::query_parameters::UpdateNodeOptionsBuilder::default()
        .version(version)
        .build();
    client.inner().update_node(name, spec, opts).await?;

    let label_count = new_labels.len();
    println!("Node '{}' labels updated ({} labels)", name, label_count);
    Ok(())
}

async fn update_secret_labels(
    client: &DockerClient,
    name: &str,
    labels: Vec<String>,
    overwrite: bool,
    delete_all: bool,
) -> anyhow::Result<()> {
    let secrets = crate::api::secret::list_secrets(client.inner()).await?;
    let secret = secrets
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Secret '{}' not found", name))?;

    let version = secret.version.and_then(|v| v.index).unwrap_or(0) as i64;
    let mut spec = secret.spec.unwrap_or_default();

    let new_labels = if delete_all {
        HashMap::new()
    } else if overwrite {
        parse_labels(&labels)
    } else {
        let mut existing = spec.labels.unwrap_or_default();
        for (k, v) in parse_labels(&labels) {
            existing.insert(k, v);
        }
        existing
    };

    spec.labels = if new_labels.is_empty() {
        None
    } else {
        Some(new_labels.clone())
    };

    let opts = bollard::query_parameters::UpdateSecretOptionsBuilder::default()
        .version(version)
        .build();
    client.inner().update_secret(name, spec, opts).await?;

    let label_count = new_labels.len();
    println!("Secret '{}' labels updated ({} labels)", name, label_count);
    Ok(())
}

async fn update_config_labels(
    client: &DockerClient,
    name: &str,
    labels: Vec<String>,
    overwrite: bool,
    delete_all: bool,
) -> anyhow::Result<()> {
    let configs = crate::api::config::list_configs(client.inner()).await?;
    let config = configs
        .into_iter()
        .find(|c| c.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Config '{}' not found", name))?;

    let version = config.version.and_then(|v| v.index).unwrap_or(0) as i64;
    let mut spec = config.spec.unwrap_or_default();

    let new_labels = if delete_all {
        HashMap::new()
    } else if overwrite {
        parse_labels(&labels)
    } else {
        let mut existing = spec.labels.unwrap_or_default();
        for (k, v) in parse_labels(&labels) {
            existing.insert(k, v);
        }
        existing
    };

    spec.labels = if new_labels.is_empty() {
        None
    } else {
        Some(new_labels.clone())
    };

    let opts = bollard::query_parameters::UpdateConfigOptionsBuilder::default()
        .version(version)
        .build();
    client.inner().update_config(name, spec, opts).await?;

    let label_count = new_labels.len();
    println!("Config '{}' labels updated ({} labels)", name, label_count);
    Ok(())
}
