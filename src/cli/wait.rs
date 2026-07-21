use crate::api::client::DockerClient;

pub async fn run(
    client: &DockerClient,
    resource: &str,
    name: String,
    condition: String,
    timeout_secs: u64,
) -> anyhow::Result<()> {
    let resource_type = resolve_resource_type(resource)?;
    let start = std::time::Instant::now();

    loop {
        if start.elapsed().as_secs() > timeout_secs {
            return Err(anyhow::anyhow!(
                "Timed out waiting for condition '{}' on {} '{}'",
                condition,
                resource_type,
                name
            ));
        }

        let ready = match resource_type {
            "services" | "svc" => check_service_ready(client, &name, &condition).await?,
            "tasks" | "po" => check_task_ready(client, &name, &condition).await?,
            "nodes" | "no" => check_node_ready(client, &name, &condition).await?,
            _ => return Err(anyhow::anyhow!("wait is not supported for {}", resource)),
        };

        if ready {
            println!(
                "Condition '{}' met for {} '{}'",
                condition, resource_type, name
            );
            return Ok(());
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

fn resolve_resource_type(s: &str) -> anyhow::Result<&'static str> {
    match s.to_lowercase().as_str() {
        "services" | "svc" | "service" => Ok("services"),
        "tasks" | "po" | "task" => Ok("tasks"),
        "nodes" | "no" | "node" => Ok("nodes"),
        _ => Err(anyhow::anyhow!("Unknown resource type: {}", s)),
    }
}

async fn check_service_ready(
    client: &DockerClient,
    name: &str,
    condition: &str,
) -> anyhow::Result<bool> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let service = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name.to_string()));

    match service {
        Some(s) => {
            let spec = s.spec.as_ref();
            let mode = spec.and_then(|sp| sp.mode.as_ref());
            let desired = mode
                .and_then(|m| m.replicated.as_ref())
                .and_then(|r| r.replicas)
                .unwrap_or(0);

            let mut filters = std::collections::HashMap::new();
            filters.insert("service".to_string(), vec![name.to_string()]);
            let task_opts = bollard::query_parameters::ListTasksOptions {
                filters: Some(filters),
            };
            let tasks = client.inner().list_tasks(Some(task_opts)).await?;
            let running = tasks
                .iter()
                .filter(|t| {
                    matches!(
                        t.desired_state
                            .unwrap_or(bollard::models::TaskState::RUNNING),
                        bollard::models::TaskState::RUNNING
                    )
                })
                .count() as i64;

            match condition {
                "available" | "ready" | "Running" => Ok(running >= desired),
                "exists" => Ok(true),
                _ => Ok(running >= desired),
            }
        }
        None => {
            if condition == "deleted" || condition == "removed" {
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }
}

async fn check_task_ready(
    client: &DockerClient,
    name: &str,
    condition: &str,
) -> anyhow::Result<bool> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;
    let task = tasks
        .into_iter()
        .find(|t| t.id.as_deref() == Some(name) || t.name.as_deref() == Some(name));

    match task {
        Some(t) => {
            let state = t.status.as_ref().and_then(|s| s.state);
            match condition {
                "running" | "Running" => {
                    Ok(matches!(state, Some(bollard::models::TaskState::RUNNING)))
                }
                "complete" | "Completed" => {
                    Ok(matches!(state, Some(bollard::models::TaskState::COMPLETE)))
                }
                "exists" => Ok(true),
                _ => Ok(true),
            }
        }
        None => {
            if condition == "deleted" || condition == "removed" {
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }
}

async fn check_node_ready(
    client: &DockerClient,
    name: &str,
    condition: &str,
) -> anyhow::Result<bool> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;
    let node = nodes
        .into_iter()
        .find(|n| n.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name.to_string()));

    match node {
        Some(n) => {
            let status = n.status.as_ref().and_then(|s| s.state);
            match condition {
                "ready" | "Ready" => Ok(matches!(status, Some(bollard::models::NodeState::READY))),
                "exists" => Ok(true),
                _ => Ok(true),
            }
        }
        None => {
            if condition == "deleted" || condition == "removed" {
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }
}
