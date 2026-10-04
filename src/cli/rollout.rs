use crate::api::client::DockerClient;

pub async fn run_status(client: &DockerClient, service_name: String) -> anyhow::Result<()> {
    let service = crate::api::service::find_service(client.inner(), &service_name).await?;

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
    let existing = crate::api::service::find_service(client.inner(), &service_name).await?;

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
    let _existing = crate::api::service::find_service(client.inner(), &service_name).await?;

    // Previously this warned on stderr and returned Ok(()), so a CI script like
    // `rollout pause svc && deploy.sh` sailed through with exit 0.
    anyhow::bail!(
        "Service rollout pause is not implemented: the Docker Engine API has no \
         endpoint for pausing a service rollout.\n\
         \n\
         To hold a rollout, set the service to the desired replica count first and \
         scale down the others, or cordon the target nodes."
    )
}

pub async fn run_resume(client: &DockerClient, service_name: String) -> anyhow::Result<()> {
    let _existing = crate::api::service::find_service(client.inner(), &service_name).await?;

    // Previously this warned on stderr and returned Ok(()), so a CI script like
    // `rollout pause svc && deploy.sh` sailed through with exit 0.
    anyhow::bail!(
        "Service rollout resume is not implemented: the Docker Engine API has no \
         endpoint for pausing a service rollout.\n\
         \n\
         To hold a rollout, set the service to the desired replica count first and \
         scale down the others, or cordon the target nodes."
    )
}

pub async fn run_restart(client: &DockerClient, service_name: String) -> anyhow::Result<()> {
    let existing = crate::api::service::find_service(client.inner(), &service_name).await?;

    let version = existing.version.and_then(|v| v.index).unwrap_or(0) as i32;
    let mut spec = existing.spec.clone().unwrap_or_default();

    // A restart is a redeploy, not a scale change. The previous implementation
    // incremented `replicas`, so restarting a 3-replica service left it running
    // 4 replicas permanently.
    //
    // Bollard's `UpdateServiceOptions` has no `force` field, so the redeploy is
    // requested the Docker-native way: bumping `Spec.TaskTemplate.ForceUpdate`
    // makes the scheduler replace running tasks without touching desired state.
    if let Some(template) = spec.task_template.as_mut() {
        template.force_update = Some(template.force_update.unwrap_or(0) + 1);
    } else {
        anyhow::bail!(
            "Service '{}' has no task template to redeploy",
            service_name
        );
    }

    let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
        .version(version)
        .build();

    client
        .inner()
        .update_service(&service_name, spec, opts, None)
        .await?;
    println!("Service '{}' restart initiated", service_name);

    // Count tasks by their *actual* state. Reading desired_state here would
    // report the pre-restart situation, since the redeploy is asynchronous.
    let mut filters = std::collections::HashMap::new();
    filters.insert("service".to_string(), vec![service_name.clone()]);
    let task_opts = bollard::query_parameters::ListTasksOptions {
        filters: Some(filters),
    };
    let tasks = client.inner().list_tasks(Some(task_opts)).await?;
    let running_count = tasks
        .iter()
        .filter(|t| crate::api::task::task_is_running(t))
        .count();
    println!("  -> {} running tasks", running_count);

    Ok(())
}
