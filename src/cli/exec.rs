use crate::api::client::DockerClient;
use bollard::exec::CreateExecOptions;
use bollard::exec::StartExecResults;
use futures::StreamExt;

pub async fn run(
    client: &DockerClient,
    service_name: String,
    command: Vec<String>,
) -> anyhow::Result<()> {
    let (_task_id, container_id) =
        crate::api::task::running_container_for_service(client, &service_name).await?;

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
