use crate::api::client::DockerClient;
use crate::cli::root::ResourceType;
use crate::utils::selectors::matches_selector;

/// Flags accepted by every resource-specific delete handler.
///
/// `force`, `timeout` and `wait_for_delete` are parsed and threaded through but
/// not yet honoured by the handlers; they are grouped here so the gap is visible
/// in one place rather than as five underscore-prefixed parameters per handler.
#[derive(Clone, Default)]
pub struct DeleteOptions {
    pub selector: Option<String>,
    pub dry_run: bool,
    pub ignore_not_found: bool,
    pub grace_period: Option<i64>,
    #[allow(dead_code)]
    pub force: bool,
    #[allow(dead_code)]
    pub timeout: Option<u64>,
    #[allow(dead_code)]
    pub wait_for_delete: bool,
}

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: Option<String>,
    opts: DeleteOptions,
) -> anyhow::Result<()> {
    let DeleteOptions {
        selector,
        dry_run,
        ignore_not_found,
        grace_period,
        ..
    } = opts;

    if let Some(grace) = grace_period {
        // `grace as u64` used to turn -5 into a ~584-billion-year sleep.
        if grace < 0 {
            anyhow::bail!("--grace-period must not be negative (got {})", grace);
        }
        // Only services have containers with a shutdown grace period.
        let grace_applies = matches!(
            resource,
            ResourceType::Services | ResourceType::Tasks | ResourceType::Nodes
        );
        if !grace_applies {
            eprintln!(
                "warning: --grace-period has no effect on {} (no containers to drain)",
                format!("{:?}", resource).to_lowercase()
            );
        }
    }
    match resource {
        ResourceType::Services => {
            delete_service(
                client,
                name,
                selector,
                dry_run,
                ignore_not_found,
                grace_period,
            )
            .await?
        }
        ResourceType::Networks => {
            delete_network(
                client,
                name,
                selector,
                dry_run,
                ignore_not_found,
                grace_period,
            )
            .await?
        }
        ResourceType::Secrets => {
            delete_secret(
                client,
                name,
                selector,
                dry_run,
                ignore_not_found,
                grace_period,
            )
            .await?
        }
        ResourceType::Configs => {
            delete_config(
                client,
                name,
                selector,
                dry_run,
                ignore_not_found,
                grace_period,
            )
            .await?
        }
        ResourceType::Tasks => delete_task(name, ignore_not_found).await?,
        ResourceType::Nodes => {
            delete_node(
                client,
                name,
                selector,
                dry_run,
                ignore_not_found,
                grace_period,
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

/// Give a service's containers `grace` seconds to shut down cleanly.
///
/// `--grace-period` used to sleep in the *client* before deleting, which both
/// blocked the caller and achieved nothing: the delete that followed was
/// immediate, so containers still received Docker's default 10s. The value
/// belongs in `Spec.TaskTemplate.StopGracePeriod`, where the daemon applies it.
async fn apply_grace_period(client: &DockerClient, name: &str, grace: i64) -> anyhow::Result<()> {
    if grace < 0 {
        anyhow::bail!("--grace-period must not be negative (got {})", grace);
    }

    let services = crate::api::service::list_services(client.inner()).await?;
    let Some(service) = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_deref()) == Some(name))
    else {
        // Nothing to configure; the delete below will report the real error.
        return Ok(());
    };

    let version = service.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i32;
    let mut spec = service.spec.clone().unwrap_or_default();
    match spec
        .task_template
        .as_mut()
        .and_then(|t| t.container_spec.as_mut())
    {
        Some(c) => c.stop_grace_period = Some(grace),
        None => {
            log::warn!(
                "Service '{}' has no task template; --grace-period cannot be applied",
                name
            );
            return Ok(());
        }
    }

    let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
        .version(version)
        .build();
    client
        .inner()
        .update_service(name, spec, opts, None)
        .await?;
    Ok(())
}

async fn delete_service(
    client: &DockerClient,
    name: Option<String>,
    selector: Option<String>,
    dry_run: bool,
    ignore_not_found: bool,
    grace_period: Option<i64>,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        if dry_run {
            println!("[dry run] Would delete service '{}'", name);
            return Ok(());
        }
        if let Some(grace) = grace_period {
            apply_grace_period(client, &name, grace).await?;
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
                apply_grace_period(client, name, grace).await?;
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
    dry_run: bool,
    ignore_not_found: bool,
    _grace_period: Option<i64>,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        if dry_run {
            println!("[dry run] Would delete network '{}'", name);
            return Ok(());
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
    dry_run: bool,
    ignore_not_found: bool,
    _grace_period: Option<i64>,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        if dry_run {
            println!("[dry run] Would delete secret '{}'", name);
            return Ok(());
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
    dry_run: bool,
    ignore_not_found: bool,
    _grace_period: Option<i64>,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        if dry_run {
            println!("[dry run] Would delete config '{}'", name);
            return Ok(());
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
            client.inner().delete_config(id).await?;
            println!("config/{} deleted", name);
        }
    } else {
        return Err(anyhow::anyhow!("Must specify config name or selector"));
    }

    Ok(())
}

async fn delete_task(name: Option<String>, ignore_not_found: bool) -> anyhow::Result<()> {
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
    dry_run: bool,
    ignore_not_found: bool,
    _grace_period: Option<i64>,
) -> anyhow::Result<()> {
    if let Some(name) = name {
        if dry_run {
            println!("[dry run] Would drain node '{}'", name);
            return Ok(());
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
            return Err(anyhow::anyhow!("No nodes matching selector '{}'", selector));
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
            let version = node.version.and_then(|v| v.index).unwrap_or(0) as i64;
            let mut spec = node.spec.unwrap_or_default();
            spec.availability = Some(bollard::models::NodeSpecAvailabilityEnum::DRAIN);
            let opts = bollard::query_parameters::UpdateNodeOptionsBuilder::default()
                .version(version)
                .build();
            client.inner().update_node(&node_name, spec, opts).await?;
            println!("node/{} drained", node_name);
        }
    } else {
        return Err(anyhow::anyhow!("Must specify node name or selector"));
    }

    Ok(())
}
