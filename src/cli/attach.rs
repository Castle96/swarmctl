use crate::api::client::DockerClient;
use bollard::query_parameters::AttachContainerOptions;
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn run(
    client: &DockerClient,
    service_name: String,
    interactive: bool,
) -> anyhow::Result<()> {
    let (_task_id, container_id) =
        crate::api::task::running_container_for_service(client, &service_name).await?;

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
