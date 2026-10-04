use crate::api::client::DockerClient;
use tokio::net::{TcpListener, TcpStream};

pub async fn run(
    client: &DockerClient,
    service_name: String,
    local_port: u16,
    container_port: u16,
) -> anyhow::Result<()> {
    let (_task_id, container_id) =
        crate::api::task::running_container_for_service(client, &service_name).await?;

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
                        && e.kind() != std::io::ErrorKind::ConnectionReset
                    {
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
