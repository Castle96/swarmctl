use crate::api::client::DockerClient;

pub async fn run_status(client: &DockerClient, service_name: String) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let service = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&service_name))
        .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", service_name))?;

    let spec = service.spec.as_ref();
    let name = spec.and_then(|s| s.name.clone()).unwrap_or_default();
    let mode = spec.and_then(|s| s.mode.as_ref());
    let replicas = mode
        .and_then(|m| m.replicated.as_ref())
        .and_then(|r| r.replicas)
        .unwrap_or(0);
    let update_config = spec.and_then(|s| s.update_config.as_ref());

    println!("Service:       {}", name);
    println!("Replicas:      {}", replicas);
    if let Some(uc) = update_config {
        println!("Update Params:");
        println!("  Parallelism: {}", uc.parallelism.unwrap_or(0));
        println!("  Delay:       {}s", uc.delay.unwrap_or(0));
        if let Some(failure) = &uc.failure_action {
            println!("  Failure:     {}", failure);
        }
        if let Some(order) = &uc.order {
            println!("  Order:       {}", order);
        }
    }

    let mut filters = std::collections::HashMap::new();
    filters.insert("service".to_string(), vec![service_name.clone()]);
    let task_opts = bollard::query_parameters::ListTasksOptions {
        filters: Some(filters),
    };
    let tasks = client.inner().list_tasks(Some(task_opts)).await?;

    let mut running = 0u64;
    let mut pending = 0u64;
    let mut failed = 0u64;
    let mut shutdown = 0u64;
    for t in &tasks {
        match t
            .desired_state
            .unwrap_or(bollard::models::TaskState::RUNNING)
        {
            bollard::models::TaskState::RUNNING => running += 1,
            bollard::models::TaskState::PENDING => pending += 1,
            bollard::models::TaskState::FAILED => failed += 1,
            _ => shutdown += 1,
        }
    }

    println!("\nTask Summary:");
    println!("  Running:  {}", running);
    println!("  Pending:  {}", pending);
    println!("  Failed:   {}", failed);
    println!("  Shutdown: {}", shutdown);
    println!("  Total:    {}", tasks.len());

    Ok(())
}

pub async fn run_history(client: &DockerClient, service_name: String) -> anyhow::Result<()> {
    let mut filters = std::collections::HashMap::new();
    filters.insert("service".to_string(), vec![service_name.clone()]);
    let task_opts = bollard::query_parameters::ListTasksOptions {
        filters: Some(filters),
    };
    let tasks = client.inner().list_tasks(Some(task_opts)).await?;

    if tasks.is_empty() {
        println!("No tasks found for service '{}'", service_name);
        return Ok(());
    }

    println!(
        "{:<20} {:<25} {:<20} {:<15} {:<20}",
        "TASK ID", "NAME", "STATE", "NODE", "UPDATED"
    );
    println!("{}", "-".repeat(100));

    let mut sorted = tasks;
    sorted.sort_by(|a, b| {
        let a_time = a
            .status
            .as_ref()
            .and_then(|s| s.timestamp.as_ref())
            .unwrap_or(&"".to_string())
            .clone();
        let b_time = b
            .status
            .as_ref()
            .and_then(|s| s.timestamp.as_ref())
            .unwrap_or(&"".to_string())
            .clone();
        a_time.cmp(&b_time)
    });

    for t in sorted {
        let id = t.id.as_deref().unwrap_or("").to_string();
        let short_id = if id.len() > 12 { &id[..12] } else { &id };
        let name = t.name.as_deref().unwrap_or("-").to_string();
        let state = t
            .status
            .as_ref()
            .and_then(|s| s.state)
            .map(|v| format!("{:?}", v))
            .unwrap_or_default();
        let node = t.node_id.as_deref().unwrap_or("-").to_string();
        let short_node = if node.len() > 12 { &node[..12] } else { &node };
        let updated = t
            .status
            .as_ref()
            .and_then(|s| s.timestamp.as_ref())
            .map(|t| &t[..19])
            .unwrap_or("-");

        println!(
            "{:<20} {:<25} {:<20} {:<15} {:<20}",
            short_id, name, state, short_node, updated
        );
    }

    Ok(())
}

pub async fn run_undo(client: &DockerClient, service_name: String) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let existing = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&service_name))
        .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", service_name))?;

    let spec = existing.spec.unwrap_or_default();
    let version = existing.version.and_then(|v| v.index).unwrap_or(0) as i32;

    let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
        .version(version)
        .rollback("previous")
        .build();
    client
        .inner()
        .update_service(&service_name, spec, opts, None)
        .await?;
    println!("Service '{}' rolled back to previous version", service_name);
    Ok(())
}

pub async fn run_pause(client: &DockerClient, service_name: String) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let _existing = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&service_name))
        .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", service_name))?;

    eprintln!("Warning: Service rollout pause is not supported by the Docker API in this bollard version");
    Ok(())
}

pub async fn run_resume(client: &DockerClient, service_name: String) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let _existing = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&service_name))
        .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", service_name))?;

    eprintln!("Warning: Service rollout resume is not supported by the Docker API in this bollard version");
    Ok(())
}

pub async fn run_restart(client: &DockerClient, service_name: String) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let existing = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&service_name))
        .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", service_name))?;

    let version = existing.version.and_then(|v| v.index).unwrap_or(0) as i32;
    let mut spec = existing.spec.unwrap_or_default();

    if let Some(mode) = &mut spec.mode
        && let Some(ref mut replicated) = mode.replicated {
            let current = replicated.replicas.unwrap_or(0);
            replicated.replicas = Some(current + 1);
        }

    let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
        .version(version)
        .build();

    client
        .inner()
        .update_service(&service_name, spec, opts, None)
        .await?;
    println!("Service '{}' restart initiated", service_name);

    let mut filters = std::collections::HashMap::new();
    filters.insert("service".to_string(), vec![service_name.clone()]);
    let task_opts = bollard::query_parameters::ListTasksOptions {
        filters: Some(filters.clone()),
    };
    let tasks = client.inner().list_tasks(Some(task_opts)).await?;
    let running_count = tasks
        .iter()
        .filter(|t| {
            matches!(
                t.desired_state
                    .unwrap_or(bollard::models::TaskState::RUNNING),
                bollard::models::TaskState::RUNNING
            )
        })
        .count();
    println!("  -> {} running tasks", running_count);

    Ok(())
}
