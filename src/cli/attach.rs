use crate::api::client::DockerClient;
use bollard::query_parameters::AttachContainerOptions;
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn run(
    client: &DockerClient,
    service_name: String,
    interactive: bool,
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

    let options = AttachContainerOptions {
        stdout: true,
        stderr: true,
        stream: true,
        logs: false,
        stdin: interactive,
        ..Default::default()
    };

    let results = client
        .inner()
        .attach_container(&container_id, Some(options))
        .await?;
    let mut output = results.output;

    if interactive {
        let mut input = results.input;
        let output_handle = tokio::spawn(async move {
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
                        eprintln!("Attach error: {}", e);
                        break;
                    }
                }
            }
        });

        let input_handle = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            loop {
                match tokio::io::stdin().read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Err(e) =
                            tokio::io::AsyncWriteExt::write_all(&mut input, &buf[..n]).await
                        {
                            eprintln!("Input error: {}", e);
                            break;
                        }
                        let _ = input.flush().await;
                    }
                    Err(e) => {
                        eprintln!("Stdin error: {}", e);
                        break;
                    }
                }
            }
        });

        let _ = tokio::try_join!(output_handle, input_handle);
    } else {
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
                    eprintln!("Attach error: {}", e);
                    break;
                }
            }
        }
    }

    Ok(())
}
