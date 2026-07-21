use crate::api::client::DockerClient;
use crate::cli::root::LogResourceType;
use futures::StreamExt;

pub async fn run(
    client: &DockerClient,
    resource: LogResourceType,
    name: String,
    follow: bool,
    tail: i64,
    previous: bool,
) -> anyhow::Result<()> {
    match resource {
        LogResourceType::Service => logs_service(client, name, follow, tail, previous).await?,
        LogResourceType::Task => logs_task(client, name, follow, tail).await?,
    }

    Ok(())
}

async fn logs_service(
    client: &DockerClient,
    name: String,
    follow: bool,
    tail: i64,
    previous: bool,
) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let _service = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|spec| spec.name.as_ref()) == Some(&name))
        .ok_or_else(|| anyhow::anyhow!("Service {} not found", name))?;

    if previous {
        let mut filters = std::collections::HashMap::new();
        filters.insert("service".to_string(), vec![name.clone()]);
        filters.insert(
            "desired-state".to_string(),
            vec!["shutdown".to_string(), "failed".to_string()],
        );

        let options = bollard::query_parameters::ListTasksOptions {
            filters: Some(filters),
        };
        let tasks = client.inner().list_tasks(Some(options)).await?;

        if tasks.is_empty() {
            println!("No previous tasks found for service '{}'", name);
            return Ok(());
        }

        for task in &tasks {
            let task_id = task.id.as_deref().unwrap_or("unknown");
            let task_name = task.name.as_deref().unwrap_or(task_id);
            println!("--- Previous task: {} (ID: {}) ---", task_name, task_id);

            let log_opts = bollard::query_parameters::LogsOptions {
                follow: false,
                stdout: true,
                stderr: true,
                tail: if tail > 0 {
                    tail.to_string()
                } else {
                    "all".to_string()
                },
                ..Default::default()
            };

            let mut logs = client.inner().task_logs(task_id, Some(log_opts));
            while let Some(log_result) = logs.next().await {
                match log_result {
                    Ok(log) => print!("{}", log),
                    Err(e) => eprintln!("Error reading task logs: {}", e),
                }
            }
        }
        return Ok(());
    }

    let options = bollard::query_parameters::LogsOptions {
        follow,
        stdout: true,
        stderr: true,
        tail: if tail > 0 {
            tail.to_string()
        } else {
            "all".to_string()
        },
        ..Default::default()
    };

    let mut logs = client.inner().service_logs(&name, Some(options));

    while let Some(log_result) = logs.next().await {
        match log_result {
            Ok(log) => print!("{}", log),
            Err(e) => {
                eprintln!("Error reading logs: {}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn logs_task(
    client: &DockerClient,
    task_id: String,
    follow: bool,
    tail: i64,
) -> anyhow::Result<()> {
    let options = bollard::query_parameters::LogsOptions {
        follow,
        stdout: true,
        stderr: true,
        tail: if tail > 0 {
            tail.to_string()
        } else {
            "all".to_string()
        },
        ..Default::default()
    };

    let mut logs = client.inner().task_logs(&task_id, Some(options));

    while let Some(log_result) = logs.next().await {
        match log_result {
            Ok(log) => print!("{}", log),
            Err(e) => {
                eprintln!("Error reading task logs: {}", e);
                break;
            }
        }
    }

    Ok(())
}
