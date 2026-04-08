use crate::api::client::DockerClient;
use crate::cli::root::LogResourceType;
use futures::StreamExt;

pub async fn run(
    client: &DockerClient,
    resource: LogResourceType,
    name: String,
    follow: bool,
    tail: i64,
) -> anyhow::Result<()> {
    match resource {
        LogResourceType::Service => logs_service(client, name, follow, tail).await?,
        LogResourceType::Task => logs_task(client, name, follow, tail).await?,
    }

    Ok(())
}

async fn logs_service(client: &DockerClient, name: String, follow: bool, tail: i64) -> anyhow::Result<()> {
    // Find the service
    let services = crate::api::service::list_services(client.inner()).await?;
    let _service = services.into_iter()
        .find(|s| s.spec.as_ref().and_then(|spec| spec.name.as_ref()) == Some(&name))
        .ok_or_else(|| anyhow::anyhow!("Service {} not found", name))?;

    // Get service logs
    let options = bollard::query_parameters::LogsOptions {
        follow,
        stdout: true,
        stderr: true,
        tail: if tail > 0 { tail.to_string() } else { "all".to_string() },
        ..Default::default()
    };

    let mut logs = client.inner().service_logs(&name, Some(options));

    while let Some(log_result) = logs.next().await {
        match log_result {
            Ok(log) => {
                print!("{}", log);
            }
            Err(e) => {
                eprintln!("Error reading logs: {}", e);
                break;
            }
        }
    }

    Ok(())
}

async fn logs_task(client: &DockerClient, task_id: String, follow: bool, tail: i64) -> anyhow::Result<()> {
    // Get task logs
    let options = bollard::query_parameters::LogsOptions {
        follow,
        stdout: true,
        stderr: true,
        tail: if tail > 0 { tail.to_string() } else { "all".to_string() },
        ..Default::default()
    };

    let mut logs = client.inner().task_logs(&task_id, Some(options));

    while let Some(log_result) = logs.next().await {
        match log_result {
            Ok(log) => {
                print!("{}", log);
            }
            Err(e) => {
                eprintln!("Error reading task logs: {}", e);
                break;
            }
        }
    }

    Ok(())
}