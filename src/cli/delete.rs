use crate::api::client::DockerClient;
use crate::cli::root::ResourceType;
use crate::utils::selectors::matches_selector;

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: Option<String>,
    selector: Option<String>,
    force: bool,
    dry_run: bool,
    ignore_not_found: bool,
    grace_period: Option<i64>,
    timeout: Option<u64>,
    wait_for_delete: bool,
) -> anyhow::Result<()> {
    match resource {
        ResourceType::Services => {
            delete_service(
                client,
                name,
                selector,
                force,
                dry_run,
                ignore_not_found,
                grace_period,
                timeout,
                wait_for_delete,
            )
            .await?
        }
        ResourceType::Networks => {
            delete_network(
                client,
                name,
                selector,
                force,
                dry_run,
                ignore_not_found,
                grace_period,
                timeout,
            )
            .await?
        }
        ResourceType::Secrets => {
            delete_secret(
                client,
                name,
                selector,
                force,
                dry_run,
                ignore_not_found,
                grace_period,
                timeout,
            )
            .await?
        }
        ResourceType::Configs => {
            delete_config(
                client,
                name,
                selector,
                force,
                dry_run,
                ignore_not_found,
                grace_period,
                timeout,
            )
            .await?
        }
        ResourceType::Tasks => {
            delete_task(client, name, selector, force, dry_run, ignore_not_found).await?
        }
        ResourceType::Nodes => {
            delete_node(
                client,
                name,
                selector,
                force,
                dry_run,
                ignore_not_found,
                grace_period,
                timeout,
            )
            .await?
        }
        _ => {
            if ignore_not_found {
                return Ok(());
            }
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
    dry_run: bool,
    ignore_not_found: bool,
    grace_period: Option<i64>,
    _timeout: Option<u64>,
    _wait_for_delete: bool,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        if dry_run {
            println!("[dry run] Would delete service '{}'", name);
            return Ok(());
        }
        if let Some(grace) = grace_period {
            tokio::time::sleep(std::time::Duration::from_secs(grace as u64)).await;
        }
        match client.inner().delete_service(&name).await {
            Ok(_) => println!("service/{} deleted", name),
            Err(e) => {
                if ignore_not_found && e.to_string().contains("not found") {
                    return Ok(());
                }
                return Err(e.into());
            }
        }
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
        if to_delete.is_empty() && !ignore_not_found {
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
            if dry_run {
                println!("[dry run] Would delete service '{}'", name);
                continue;
            }
            if let Some(grace) = grace_period {
                tokio::time::sleep(std::time::Duration::from_secs(grace as u64)).await;
            }
            client.inner().delete_service(id).await?;
            println!("service/{} deleted", name);
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
    dry_run: bool,
    ignore_not_found: bool,
    grace_period: Option<i64>,
    _timeout: Option<u64>,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        if dry_run {
            println!("[dry run] Would delete network '{}'", name);
            return Ok(());
        }
        if let Some(grace) = grace_period {
            tokio::time::sleep(std::time::Duration::from_secs(grace as u64)).await;
        }
        match client.inner().remove_network(&name).await {
            Ok(_) => println!("network/{} deleted", name),
            Err(e) => {
                if ignore_not_found && e.to_string().contains("not found") {
                    return Ok(());
                }
                return Err(e.into());
            }
        }
    } else if let Some(selector) = selector {
        let networks = crate::api::network::list_networks(client.inner()).await?;
        let to_delete: Vec<_> = networks
            .into_iter()
            .filter(|n| matches_selector(&n.labels.clone(), &selector))
            .collect();
        if to_delete.is_empty() && !ignore_not_found {
            return Err(anyhow::anyhow!(
                "No networks matching selector '{}'",
                selector
            ));
        }
        for network in to_delete {
            let id = network.id.as_deref().unwrap_or("unknown");
            let name = network.name.as_deref().unwrap_or(id);
            if dry_run {
                println!("[dry run] Would delete network '{}'", name);
                continue;
            }
            if let Some(grace) = grace_period {
                tokio::time::sleep(std::time::Duration::from_secs(grace as u64)).await;
            }
            client.inner().remove_network(id).await?;
            println!("network/{} deleted", name);
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
    dry_run: bool,
    ignore_not_found: bool,
    grace_period: Option<i64>,
    _timeout: Option<u64>,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        if dry_run {
            println!("[dry run] Would delete secret '{}'", name);
            return Ok(());
        }
        if let Some(grace) = grace_period {
            tokio::time::sleep(std::time::Duration::from_secs(grace as u64)).await;
        }
        match client.inner().delete_secret(&name).await {
            Ok(_) => println!("secret/{} deleted", name),
            Err(e) => {
                if ignore_not_found && e.to_string().contains("not found") {
                    return Ok(());
                }
                return Err(e.into());
            }
        }
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
        if to_delete.is_empty() && !ignore_not_found {
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
            if dry_run {
                println!("[dry run] Would delete secret '{}'", name);
                continue;
            }
            if let Some(grace) = grace_period {
                tokio::time::sleep(std::time::Duration::from_secs(grace as u64)).await;
            }
            client.inner().delete_secret(id).await?;
            println!("secret/{} deleted", name);
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
    dry_run: bool,
    ignore_not_found: bool,
    grace_period: Option<i64>,
    _timeout: Option<u64>,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        if dry_run {
            println!("[dry run] Would delete config '{}'", name);
            return Ok(());
        }
        if let Some(grace) = grace_period {
            tokio::time::sleep(std::time::Duration::from_secs(grace as u64)).await;
        }
        match client.inner().delete_config(&name).await {
            Ok(_) => println!("config/{} deleted", name),
            Err(e) => {
                if ignore_not_found && e.to_string().contains("not found") {
                    return Ok(());
                }
                return Err(e.into());
            }
        }
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
        if to_delete.is_empty() && !ignore_not_found {
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
            if dry_run {
                println!("[dry run] Would delete config '{}'", name);
                continue;
            }
            if let Some(grace) = grace_period {
                tokio::time::sleep(std::time::Duration::from_secs(grace as u64)).await;
            }
            client.inner().delete_config(id).await?;
            println!("config/{} deleted", name);
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
    _dry_run: bool,
    ignore_not_found: bool,
) -> anyhow::Result<()> {
    if let Some(_task_id) = name {
        if ignore_not_found {
            return Ok(());
        }
        Err(anyhow::anyhow!(
            "Task deletion is not supported. Tasks are managed through services."
        ))
    } else {
        if ignore_not_found {
            return Ok(());
        }
        Err(anyhow::anyhow!("Must specify task ID"))
    }
}

async fn delete_node(
    client: &DockerClient,
    name: Option<String>,
    selector: Option<String>,
    _force: bool,
    dry_run: bool,
    ignore_not_found: bool,
    grace_period: Option<i64>,
    _timeout: Option<u64>,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        if dry_run {
            println!("[dry run] Would drain node '{}'", name);
            return Ok(());
        }
        if let Some(grace) = grace_period {
            tokio::time::sleep(std::time::Duration::from_secs(grace as u64)).await;
        }

        let nodes = crate::api::node::list_nodes(client.inner()).await?;
        let node = nodes
            .into_iter()
            .find(|n| n.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name));

        match node {
            Some(node) => {
                let version = node.version.and_then(|v| v.index).unwrap_or(0) as i64;
                let mut spec = node.spec.unwrap_or_default();
                spec.availability = Some(bollard::models::NodeSpecAvailabilityEnum::DRAIN);
                let opts = bollard::query_parameters::UpdateNodeOptionsBuilder::default()
                    .version(version)
                    .build();
                client.inner().update_node(&name, spec, opts).await?;
                println!("node/{} drained", name);
            }
            None => {
                if ignore_not_found {
                    return Ok(());
                }
                return Err(anyhow::anyhow!("Node '{}' not found", name));
            }
        }
    } else if let Some(selector) = selector {
        let nodes = crate::api::node::list_nodes(client.inner()).await?;
        let to_delete: Vec<_> = nodes
            .into_iter()
            .filter(|n| {
                matches_selector(&n.spec.as_ref().and_then(|sp| sp.labels.clone()), &selector)
            })
            .collect();
        if to_delete.is_empty() && !ignore_not_found {
            return Err(anyhow::anyhow!(
                "No nodes matching selector '{}'",
                selector
            ));
        }
        for node in to_delete {
            let node_name = node
                .spec
                .as_ref()
                .and_then(|sp| sp.name.as_deref())
                .unwrap_or("unknown")
                .to_string();
            if dry_run {
                println!("[dry run] Would drain node '{}'", node_name);
                continue;
            }
            if let Some(grace) = grace_period {
                tokio::time::sleep(std::time::Duration::from_secs(grace as u64)).await;
            }
            let version = node.version.and_then(|v| v.index).unwrap_or(0) as i64;
            let mut spec = node.spec.unwrap_or_default();
            spec.availability = Some(bollard::models::NodeSpecAvailabilityEnum::DRAIN);
            let opts = bollard::query_parameters::UpdateNodeOptionsBuilder::default()
                .version(version)
                .build();
            client
                .inner()
                .update_node(&node_name, spec, opts)
                .await?;
            println!("node/{} drained", node_name);
        }
    } else {
        return Err(anyhow::anyhow!("Must specify node name or selector"));
    }

    Ok(())
}
