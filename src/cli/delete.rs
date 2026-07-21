use crate::api::client::DockerClient;
use crate::cli::root::ResourceType;
use std::collections::HashMap;

fn matches_selector(labels: &Option<HashMap<String, String>>, selector: &str) -> bool {
    let Some((key, value)) = selector.split_once('=') else {
        return false;
    };
    labels
        .as_ref()
        .and_then(|l| l.get(key))
        .map(|v| v == value)
        .unwrap_or(false)
}

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: Option<String>,
    selector: Option<String>,
    force: bool,
) -> anyhow::Result<()> {
    match resource {
        ResourceType::Services => delete_service(client, name, selector, force).await?,
        ResourceType::Networks => delete_network(client, name, selector, force).await?,
        ResourceType::Secrets => delete_secret(client, name, selector, force).await?,
        ResourceType::Configs => delete_config(client, name, selector, force).await?,
        ResourceType::Tasks => delete_task(client, name, selector, force).await?,
        _ => {
            return Err(anyhow::anyhow!(
                "Deleting {} is not yet supported",
                format!("{:?}", resource).to_lowercase()
            ));
        }
    }

    Ok(())
}

async fn delete_service(
    client: &DockerClient,
    name: Option<String>,
    selector: Option<String>,
    _force: bool,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        println!("Deleting service {}...", name);
        client.inner().delete_service(&name).await?;
        println!("Service {} deleted", name);
    } else if let Some(selector) = selector {
        let services = crate::api::service::list_services(client.inner()).await?;
        let to_delete: Vec<_> = services
            .into_iter()
            .filter(|s| {
                matches_selector(
                    &s.spec.as_ref().and_then(|spec| spec.labels.clone()),
                    &selector,
                )
            })
            .collect();
        if to_delete.is_empty() {
            return Err(anyhow::anyhow!(
                "No services matching selector '{}'",
                selector
            ));
        }
        for service in to_delete {
            let id = service.id.as_deref().unwrap_or("unknown");
            let name = service
                .spec
                .as_ref()
                .and_then(|s| s.name.as_deref())
                .unwrap_or(id);
            println!("Deleting service {}...", name);
            client.inner().delete_service(id).await?;
            println!("Service {} deleted", name);
        }
    } else {
        return Err(anyhow::anyhow!("Must specify service name or selector"));
    }

    Ok(())
}

async fn delete_network(
    client: &DockerClient,
    name: Option<String>,
    selector: Option<String>,
    _force: bool,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        println!("Deleting network {}...", name);
        client.inner().remove_network(&name).await?;
        println!("Network {} deleted", name);
    } else if let Some(selector) = selector {
        let networks = crate::api::network::list_networks(client.inner()).await?;
        let to_delete: Vec<_> = networks
            .into_iter()
            .filter(|n| matches_selector(&n.labels.clone(), &selector))
            .collect();
        if to_delete.is_empty() {
            return Err(anyhow::anyhow!(
                "No networks matching selector '{}'",
                selector
            ));
        }
        for network in to_delete {
            let id = network.id.as_deref().unwrap_or("unknown");
            let name = network.name.as_deref().unwrap_or(id);
            println!("Deleting network {}...", name);
            client.inner().remove_network(id).await?;
            println!("Network {} deleted", name);
        }
    } else {
        return Err(anyhow::anyhow!("Must specify network name or selector"));
    }

    Ok(())
}

async fn delete_secret(
    client: &DockerClient,
    name: Option<String>,
    selector: Option<String>,
    _force: bool,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        println!("Deleting secret {}...", name);
        client.inner().delete_secret(&name).await?;
        println!("Secret {} deleted", name);
    } else if let Some(selector) = selector {
        let secrets = crate::api::secret::list_secrets(client.inner()).await?;
        let to_delete: Vec<_> = secrets
            .into_iter()
            .filter(|s| {
                matches_selector(
                    &s.spec.as_ref().and_then(|spec| spec.labels.clone()),
                    &selector,
                )
            })
            .collect();
        if to_delete.is_empty() {
            return Err(anyhow::anyhow!(
                "No secrets matching selector '{}'",
                selector
            ));
        }
        for secret in to_delete {
            let id = secret.id.as_deref().unwrap_or("unknown");
            let name = secret
                .spec
                .as_ref()
                .and_then(|s| s.name.as_deref())
                .unwrap_or(id);
            println!("Deleting secret {}...", name);
            client.inner().delete_secret(id).await?;
            println!("Secret {} deleted", name);
        }
    } else {
        return Err(anyhow::anyhow!("Must specify secret name or selector"));
    }

    Ok(())
}

async fn delete_config(
    client: &DockerClient,
    name: Option<String>,
    selector: Option<String>,
    _force: bool,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        println!("Deleting config {}...", name);
        client.inner().delete_config(&name).await?;
        println!("Config {} deleted", name);
    } else if let Some(selector) = selector {
        let configs = crate::api::config::list_configs(client.inner()).await?;
        let to_delete: Vec<_> = configs
            .into_iter()
            .filter(|c| {
                matches_selector(
                    &c.spec.as_ref().and_then(|spec| spec.labels.clone()),
                    &selector,
                )
            })
            .collect();
        if to_delete.is_empty() {
            return Err(anyhow::anyhow!(
                "No configs matching selector '{}'",
                selector
            ));
        }
        for config in to_delete {
            let id = config.id.as_deref().unwrap_or("unknown");
            let name = config
                .spec
                .as_ref()
                .and_then(|s| s.name.as_deref())
                .unwrap_or(id);
            println!("Deleting config {}...", name);
            client.inner().delete_config(id).await?;
            println!("Config {} deleted", name);
        }
    } else {
        return Err(anyhow::anyhow!("Must specify config name or selector"));
    }

    Ok(())
}

async fn delete_task(
    _client: &DockerClient,
    name: Option<String>,
    _selector: Option<String>,
    _force: bool,
) -> anyhow::Result<()> {
    if let Some(task_id) = name {
        println!("Deleting task {}...", task_id);
        Err(anyhow::anyhow!(
            "Task deletion is not supported. Tasks are managed through services."
        ))
    } else {
        Err(anyhow::anyhow!("Must specify task ID"))
    }
}
