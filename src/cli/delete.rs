use crate::api::client::DockerClient;
use crate::cli::root::ResourceType;
use crate::utils::selectors::matches_selector;

/// Flags accepted by every resource-specific delete handler.
#[derive(Clone, Default)]
pub struct DeleteOptions {
    pub selector: Option<String>,
    pub dry_run: bool,
    pub ignore_not_found: bool,
    pub grace_period: Option<i64>,
    /// Bound on `--wait`, in seconds. Defaults to [`DEFAULT_WAIT_TIMEOUT`].
    pub timeout: Option<u64>,
    /// Poll until the resource is really gone before returning.
    pub wait_for_delete: bool,
}

/// Default bound on `--wait`.
pub const DEFAULT_WAIT_TIMEOUT: u64 = 60;

/// How often `--wait` re-checks.
const WAIT_POLL_SECONDS: u64 = 1;

/// Whether `name` is still present for `resource`.
async fn still_present(
    client: &DockerClient,
    resource: ResourceType,
    name: &str,
) -> anyhow::Result<bool> {
    Ok(match resource {
        ResourceType::Services => crate::api::service::find_service_opt(client.inner(), name)
            .await?
            .is_some(),
        ResourceType::Networks => crate::api::network::list_networks(client.inner())
            .await?
            .iter()
            .any(|n| n.name.as_deref() == Some(name) || n.id.as_deref() == Some(name)),
        ResourceType::Secrets => crate::api::secret::find_secret_opt(client.inner(), name)
            .await?
            .is_some(),
        ResourceType::Configs => crate::api::config::find_config_opt(client.inner(), name)
            .await?
            .is_some(),
        ResourceType::Tasks => crate::api::task::list_tasks(client.inner())
            .await?
            .iter()
            .any(|t| t.id.as_deref() == Some(name)),
        ResourceType::Nodes => crate::api::node::find_node_opt(client.inner(), name)
            .await?
            .is_some(),
        ResourceType::Stacks => crate::api::stack::list_stacks(client.inner())
            .await?
            .iter()
            .any(|s| s.name == name),
    })
}

/// Block until `name` is gone, or the timeout expires.
///
/// Docker returns from `delete_service` before the resource has actually been
/// removed, so a script that deletes and immediately re-creates races the
/// teardown. `--wait` closes that window.
pub async fn wait_until_gone(
    client: &DockerClient,
    resource: ResourceType,
    name: &str,
    timeout: Option<u64>,
) -> anyhow::Result<()> {
    let budget = timeout.unwrap_or(DEFAULT_WAIT_TIMEOUT);
    let started = std::time::Instant::now();

    loop {
        if !still_present(client, resource, name).await? {
            return Ok(());
        }
        if started.elapsed().as_secs() >= budget {
            anyhow::bail!(
                "Timed out after {}s waiting for {} '{}' to be deleted",
                budget,
                format!("{:?}", resource).to_lowercase(),
                name
            );
        }
        tokio::time::sleep(std::time::Duration::from_secs(WAIT_POLL_SECONDS)).await;
    }
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
        timeout,
        wait_for_delete,
        ..
    } = opts;

    if let Some(0) = timeout {
        anyhow::bail!("--timeout must be greater than zero seconds");
    }

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
                resource,
                name,
                selector,
                dry_run,
                ignore_not_found,
                grace_period,
                wait_for_delete,
                timeout,
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
            delete_node(client, name, selector, dry_run, ignore_not_found).await?
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

    let Some(service) = crate::api::service::find_service_opt(client.inner(), name).await? else {
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
    resource: ResourceType,
    name: Option<String>,
    selector: Option<String>,
    dry_run: bool,
    ignore_not_found: bool,
    grace_period: Option<i64>,
    wait_for_delete: bool,
    timeout: Option<u64>,
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
            Ok(_) => {
                if wait_for_delete {
                    wait_until_gone(client, resource, &name, timeout).await?;
                }
                println!("service/{} deleted", name);
            }
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
            if wait_for_delete {
                wait_until_gone(client, resource, name, timeout).await?;
            }
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
/// Drain a node.
///
/// Boltard 0.20 exposes no `remove_node` call, so a node cannot actually be
/// removed through the Docker API from here. Draining is the closest safe
/// operation: it stops new tasks being scheduled without killing the tasks
/// already running on it.
///
/// The output says "drained", not "deleted", because the node is still in the
/// swarm. `swarmctl drain` is the explicit spelling of this operation.
async fn delete_node(
    client: &DockerClient,
    name: Option<String>,
    selector: Option<String>,
    dry_run: bool,
    ignore_not_found: bool,
) -> anyhow::Result<()> {
    let targets: Vec<bollard::models::Node> = if let Some(name) = name {
        match crate::api::node::find_node_opt(client.inner(), &name).await? {
            Some(node) => vec![node],
            None => {
                if ignore_not_found {
                    return Ok(());
                }
                return Err(anyhow::anyhow!("Node '{}' not found", name));
            }
        }
    } else if let Some(selector) = selector {
        let nodes = crate::api::node::list_nodes(client.inner()).await?;
        let matched: Vec<_> = nodes
            .into_iter()
            .filter(|n| {
                matches_selector(&n.spec.as_ref().and_then(|sp| sp.labels.clone()), &selector)
            })
            .collect();
        if matched.is_empty() && !ignore_not_found {
            return Err(anyhow::anyhow!("No nodes matching selector '{}'", selector));
        }
        matched
    } else {
        return Err(anyhow::anyhow!("Must specify node name or selector"));
    };

    for node in targets {
        let node_name = node
            .spec
            .as_ref()
            .and_then(|sp| sp.name.clone())
            .or_else(|| node.description.as_ref().and_then(|d| d.hostname.clone()))
            .or_else(|| node.id.clone())
            .unwrap_or_else(|| "unknown".to_string());

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

    println!();
    println!(
        "Note: nodes are drained, not removed. Removing a node from a swarm is not \
         exposed by the Docker API client in use; drain it with `swarmctl drain` and \
         remove it with the Docker CLI if that is required."
    );

    Ok(())
}
