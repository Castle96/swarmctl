use crate::api::client::DockerClient;
use tokio::net::{TcpListener, TcpStream};

pub async fn run(
    client: &DockerClient,
    service_name: String,
    local_port: u16,
    container_port: u16,
) -> anyhow::Result<()> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;
    let task = tasks
        .into_iter()
        .find(|t| {
            let desired = t
                .desired_state
                .unwrap_or(bollard::models::TaskState::RUNNING);
            let is_running = matches!(desired, bollard::models::TaskState::RUNNING);
            is_running && t.service_id.as_deref() == Some(&service_name)
        })
        .ok_or_else(|| anyhow::anyhow!("No running tasks for service '{}'", service_name))?;

    let container_id = task
        .status
        .as_ref()
        .and_then(|s| s.container_status.as_ref())
        .and_then(|cs| cs.container_id.as_deref())
        .unwrap_or("")
        .to_string();

    if container_id.is_empty() {
        return Err(anyhow::anyhow!("Could not determine container ID"));
    }

    let container = client
        .inner()
        .inspect_container(&container_id, None)
        .await?;
    let ip = container
        .network_settings
        .as_ref()
        .and_then(|ns| ns.networks.as_ref())
        .and_then(|nets| {
            nets.values()
                .find_map(|net| net.ip_address.as_ref().filter(|ip| !ip.is_empty()))
        })
        .ok_or_else(|| anyhow::anyhow!("Could not determine container IP address"))?
        .clone();

    let bind_addr = format!("127.0.0.1:{}", local_port);
    let listener = TcpListener::bind(&bind_addr).await?;
    println!(
        "Forwarding from 127.0.0.1:{} -> {}:{} ({})",
        local_port, container_id, container_port, ip
    );
    println!("Press Ctrl+C to stop");

    loop {
        match listener.accept().await {
            Ok((local_stream, addr)) => {
                println!("Accepted connection from {}", addr);
                let target = format!("{}:{}", ip, container_port);
                tokio::spawn(async move {
                    if let Ok(remote_stream) = TcpStream::connect(&target).await
                        && let Err(e) = tokio::io::copy_bidirectional(
                            &mut tokio::io::BufStream::new(local_stream),
                            &mut tokio::io::BufStream::new(remote_stream),
                        )
                        .await
                            && e.kind() != std::io::ErrorKind::ConnectionReset {
                                eprintln!("Proxy error: {}", e);
                            }
                });
            }
            Err(e) => {
                eprintln!("Accept error: {}", e);
            }
        }
    }
}
