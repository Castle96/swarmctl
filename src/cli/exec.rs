use crate::api::client::DockerClient;
use bollard::exec::CreateExecOptions;
use bollard::exec::StartExecResults;
use futures::StreamExt;

pub async fn run(
    client: &DockerClient,
    service_name: String,
    command: Vec<String>,
) -> anyhow::Result<()> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;
    let task = tasks
        .into_iter()
        .find(|t| {
            let desired = t
                .desired_state
                .unwrap_or(bollard::models::TaskState::RUNNING);
            let is_running = matches!(desired, bollard::models::TaskState::RUNNING);
            let service_match = t.service_id.as_deref() == Some(&service_name);
            is_running && service_match
        })
        .ok_or_else(|| anyhow::anyhow!("No running tasks found for service '{}'", service_name))?;

    let container_id = task
        .status
        .as_ref()
        .and_then(|s| s.container_status.as_ref())
        .and_then(|cs| cs.container_id.as_deref())
        .unwrap_or("")
        .to_string();

    if container_id.is_empty() {
        return Err(anyhow::anyhow!("Could not determine container ID for task"));
    }

    let exec = client
        .inner()
        .create_exec(
            &container_id,
            CreateExecOptions {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                attach_stdin: Some(false),
                cmd: Some(command.clone()),
                tty: Some(false),
                ..Default::default()
            },
        )
        .await?;

    let results = client.inner().start_exec(&exec.id, None).await?;
    if let StartExecResults::Attached { output, .. } = results {
        let mut output = output;
        while let Some(result) = output.next().await {
            match result {
                Ok(log) => match log {
                    bollard::container::LogOutput::StdOut { message } => {
                        print!("{}", String::from_utf8_lossy(&message));
                    }
                    bollard::container::LogOutput::StdErr { message } => {
                        eprint!("{}", String::from_utf8_lossy(&message));
                    }
                    _ => {}
                },
                Err(e) => {
                    eprintln!("Exec error: {}", e);
                    break;
                }
            }
        }
    }

    Ok(())
}
