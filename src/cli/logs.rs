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
    timestamps: bool,
    since: Option<String>,
    prefix: bool,
    ignore_errors: bool,
) -> anyhow::Result<()> {
    match resource {
        LogResourceType::Service => {
            logs_service(client, name, follow, tail, previous, timestamps, since, prefix, ignore_errors).await?
        }
        LogResourceType::Task => {
            logs_task(client, name, follow, tail, timestamps, since, prefix, ignore_errors).await?
        }
    }

    Ok(())
}

fn parse_since(since: &str) -> anyhow::Result<i64> {
    let trimmed = since.trim();
    if let Some(s) = trimmed.strip_suffix('s') {
        let num: i64 = s.parse()?;
        return Ok(num);
    }
    if let Some(m) = trimmed.strip_suffix('m') {
        let num: i64 = m.parse()?;
        return Ok(num * 60);
    }
    if let Some(h) = trimmed.strip_suffix('h') {
        let num: i64 = h.parse()?;
        return Ok(num * 3600);
    }
    if let Some(d) = trimmed.strip_suffix('d') {
        let num: i64 = d.parse()?;
        return Ok(num * 86400);
    }
    let num: i64 = trimmed.parse()?;
    Ok(num)
}

async fn logs_service(
    client: &DockerClient,
    name: String,
    follow: bool,
    tail: i64,
    previous: bool,
    timestamps: bool,
    since: Option<String>,
    prefix: bool,
    ignore_errors: bool,
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

            let mut log_opts = bollard::query_parameters::LogsOptions {
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

            if timestamps {
                log_opts.since = since.as_deref().and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
            }

            let mut logs = client.inner().task_logs(task_id, Some(log_opts));
            while let Some(log_result) = logs.next().await {
                match log_result {
                    Ok(log) => {
                        let line = log.to_string();
                        let prefix_str = if prefix {
                            format!("[{}] ", task_name)
                        } else {
                            String::new()
                        };
                        print!("{}{}", prefix_str, line);
                    }
                    Err(e) => {
                        if ignore_errors {
                            eprintln!("Error reading task logs: {}", e);
                        } else {
                            return Err(e.into());
                        }
                    }
                }
            }
        }
        return Ok(());
    }

    let mut log_opts = bollard::query_parameters::LogsOptions {
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

    if let Some(since_str) = &since {
        let seconds = parse_since(since_str)?;
        log_opts.since = seconds as i32;
    }

    let mut logs = client.inner().service_logs(&name, Some(log_opts));

    while let Some(log_result) = logs.next().await {
        match log_result {
            Ok(log) => {
                let line = log.to_string();
                let prefix_str = if prefix {
                    format!("[{}] ", name)
                } else {
                    String::new()
                };
                print!("{}{}", prefix_str, line);
            }
            Err(e) => {
                if ignore_errors {
                    eprintln!("Error reading logs: {}", e);
                } else {
                    return Err(e.into());
                }
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
    timestamps: bool,
    since: Option<String>,
    prefix: bool,
    ignore_errors: bool,
) -> anyhow::Result<()> {
    let mut log_opts = bollard::query_parameters::LogsOptions {
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

    if let Some(since_str) = &since {
        let seconds = parse_since(since_str)?;
        log_opts.since = seconds as i32;
    }

    let mut logs = client.inner().task_logs(&task_id, Some(log_opts));

    while let Some(log_result) = logs.next().await {
        match log_result {
            Ok(log) => {
                let line = log.to_string();
                let prefix_str = if prefix {
                    format!("[{}] ", task_id)
                } else {
                    String::new()
                };
                print!("{}{}", prefix_str, line);
            }
            Err(e) => {
                if ignore_errors {
                    eprintln!("Error reading task logs: {}", e);
                } else {
                    return Err(e.into());
                }
            }
        }
    }

    Ok(())
}
