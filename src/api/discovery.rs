use anyhow::{Context, Result};
use bollard::Docker;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use super::node;

pub const SWARM_PORT: u16 = 2377;
const DOCKER_API_PORT: u16 = 2375;
const CONNECT_TIMEOUT_MS: u64 = 800;
const PROBE_TIMEOUT_MS: u64 = 2000;
const MAX_CONCURRENT_SCANS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredHost {
    pub ip: String,
    pub docker_port: Option<u16>,
    pub swarm_port: Option<u16>,
    pub docker_version: Option<String>,
    pub hostname: Option<String>,
    pub swarm_active: bool,
    pub swarm_id: Option<String>,
    pub swarm_name: Option<String>,
    pub node_count: Option<u64>,
    pub manager_count: Option<u64>,
    pub is_manager: bool,
    pub join_token_worker: Option<String>,
    pub join_token_manager: Option<String>,
}

pub fn detect_local_subnet() -> Result<Vec<Ipv4Addr>> {
    let local_ip = detect_local_ip()?;
    let octets = local_ip.octets();

    let is_private = match octets {
        [10, _, _, _] => true,
        [172, b, _, _] if (16..=31).contains(&b) => true,
        [192, 168, _, _] => true,
        _ => false,
    };

    if !is_private {
        anyhow::bail!(
            "Local IP {} is not in a private range. Cannot auto-detect subnet.",
            local_ip
        );
    }

    let mask = match octets[0] {
        10 => [255, 0, 0, 0],
        172 => [255, 255, 0, 0],
        192 => [255, 255, 255, 0],
        _ => [255, 255, 255, 0],
    };

    let network = Ipv4Addr::new(
        octets[0] & mask[0],
        octets[1] & mask[1],
        octets[2] & mask[2],
        octets[3] & mask[3],
    );

    let broadcast = Ipv4Addr::new(
        octets[0] | !mask[0],
        octets[1] | !mask[1],
        octets[2] | !mask[2],
        octets[3] | !mask[3],
    );

    let mut hosts = Vec::new();
    let mut current = u32::from_be_bytes(network.octets()) + 1;
    let end = u32::from_be_bytes(broadcast.octets()) - 1;

    while current < end {
        hosts.push(Ipv4Addr::from(current.to_be_bytes()));
        current += 1;
    }

    Ok(hosts)
}

pub fn detect_local_ip() -> Result<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("8.8.8.8:80")?;
    match socket.local_addr() {
        Ok(addr) => match addr.ip() {
            IpAddr::V4(ip) => Ok(ip),
            _ => anyhow::bail!("Local address is not IPv4"),
        },
        Err(e) => anyhow::bail!("Failed to detect local IP: {}", e),
    }
}

fn tcp_port_open(ip: &str, port: u16) -> bool {
    let addr = SocketAddr::new(ip.parse().unwrap_or(Ipv4Addr::LOCALHOST.into()), port);
    TcpStream::connect_timeout(&addr, Duration::from_millis(CONNECT_TIMEOUT_MS)).is_ok()
}

async fn probe_docker_host(ip: &str, docker_port: u16) -> Option<DiscoveredHost> {
    let docker_host = format!("tcp://{}:{}", ip, docker_port);

    let docker = match tokio::task::spawn_blocking(move || {
        Docker::connect_with_http(&docker_host, 2, bollard::API_DEFAULT_VERSION)
    })
    .await
    {
        Ok(Ok(d)) => d,
        _ => return None,
    };

    let info = match tokio::time::timeout(Duration::from_millis(PROBE_TIMEOUT_MS), docker.info())
        .await
    {
        Ok(Ok(info)) => info,
        _ => return None,
    };

    let docker_version = info
        .kernel_version
        .clone()
        .or_else(|| info.server_version.clone());

    let hostname = info.name.clone();

    let swarm_info = info.swarm.as_ref();
    let swarm_active = swarm_info
        .map(|s| {
            s.local_node_state
                .as_ref()
                .map(|state| {
                    !matches!(
                        state,
                        bollard::models::LocalNodeState::EMPTY | bollard::models::LocalNodeState::INACTIVE
                    )
                })
                .unwrap_or(false)
        })
        .unwrap_or(false);

    let mut host = DiscoveredHost {
        ip: ip.to_string(),
        docker_port: Some(docker_port),
        swarm_port: None,
        docker_version: docker_version.map(|v| v.to_string()),
        hostname: hostname.map(|h| h.to_string()),
        swarm_active,
        swarm_id: None,
        swarm_name: None,
        node_count: None,
        manager_count: None,
        is_manager: false,
        join_token_worker: None,
        join_token_manager: None,
    };

    if tcp_port_open(ip, SWARM_PORT) {
        host.swarm_port = Some(SWARM_PORT);
    }

    if swarm_active {
        if let Ok(swarm) = docker.inspect_swarm().await {
            host.swarm_id = swarm.id.clone();
            host.swarm_name = swarm.spec.as_ref().and_then(|s| s.name.clone());
        }

        if let Ok(nodes) = node::list_nodes(&docker).await {
            host.node_count = Some(nodes.len() as u64);
            host.manager_count = Some(
                nodes
                    .iter()
                    .filter(|n| {
                        n.spec
                            .as_ref()
                            .and_then(|s| s.role)
                            .map(|r| r == bollard::models::NodeSpecRoleEnum::MANAGER)
                            .unwrap_or(false)
                    })
                    .count() as u64,
            );

            let local_node_id = swarm_info.and_then(|s| s.node_id.as_deref()).unwrap_or("");
            host.is_manager = nodes.iter().any(|n| {
                n.id.as_deref() == Some(local_node_id)
                    && n.spec
                        .as_ref()
                        .and_then(|s| s.role)
                        .map(|r| r == bollard::models::NodeSpecRoleEnum::MANAGER)
                        .unwrap_or(false)
            });
        }
    }

    Some(host)
}

pub async fn probe_host_for_tokens(host: &mut DiscoveredHost) -> Result<()> {
    if !host.swarm_active {
        anyhow::bail!("Host {} is not part of a swarm", host.ip);
    }

    let docker_host = format!("tcp://{}:{}", host.ip, DOCKER_API_PORT);
    let docker = Docker::connect_with_http(&docker_host, 2, bollard::API_DEFAULT_VERSION)
        .context("Failed to connect to Docker daemon")?;

    let swarm = docker
        .inspect_swarm()
        .await
        .context("Failed to inspect swarm")?;

    if let Some(tokens) = swarm.join_tokens {
        host.join_token_worker = tokens.worker;
        host.join_token_manager = tokens.manager;
    }

    Ok(())
}

pub async fn scan_subnet(subnet: Option<&str>) -> Result<Vec<DiscoveredHost>> {
    let hosts = match subnet {
        Some(cidr) => {
            let parts: Vec<&str> = cidr.split('/').collect();
            if parts.len() != 2 {
                anyhow::bail!("Invalid CIDR format. Expected something like 192.168.1.0/24");
            }
            let base: Ipv4Addr = parts[0]
                .parse()
                .context("Invalid base IP address")?;
            let prefix_len: u8 = parts[1]
                .parse()
                .context("Invalid prefix length")?;

            if prefix_len < 16 || prefix_len > 30 {
                anyhow::bail!("Prefix length must be between 16 and 30");
            }

            let mask = !((1u32 << (32 - prefix_len)) - 1);
            let base_u32 = u32::from_be_bytes(base.octets());
            let network = base_u32 & mask;
            let broadcast = network | !mask;

            let mut result = Vec::new();
            let mut current = network + 1;
            while current < broadcast {
                result.push(Ipv4Addr::from(current.to_be_bytes()));
                current += 1;
            }
            result
        }
        None => detect_local_subnet()?,
    };

    let total = hosts.len();
    let mut discovered = Vec::new();
    let mut futures = Vec::new();

    for ip in hosts {
        let ip_str = ip.to_string();
        let fut = tokio::spawn(async move {
            let mut result = None;

            if tcp_port_open(&ip_str, DOCKER_API_PORT) {
                result = probe_docker_host(&ip_str, DOCKER_API_PORT).await;
            } else if tcp_port_open(&ip_str, SWARM_PORT) {
                let mut host = DiscoveredHost {
                    ip: ip_str.clone(),
                    docker_port: None,
                    swarm_port: Some(SWARM_PORT),
                    docker_version: None,
                    hostname: None,
                    swarm_active: false,
                    swarm_id: None,
                    swarm_name: None,
                    node_count: None,
                    manager_count: None,
                    is_manager: false,
                    join_token_worker: None,
                    join_token_manager: None,
                };

                let connect_host = format!("tcp://{}:{}", ip_str, SWARM_PORT);
                let docker_result = tokio::task::spawn_blocking(move || {
                    Docker::connect_with_http(&connect_host, 2, bollard::API_DEFAULT_VERSION)
                })
                .await;

                if let Ok(Ok(docker)) = docker_result {
                    if let Ok(info) = docker.info().await {
                        host.docker_version = info.server_version.clone();
                        host.hostname = info.name.clone();
                        let swarm_active = info
                            .swarm
                            .as_ref()
                            .and_then(|s| {
                                s.local_node_state.as_ref().map(|state| {
                                    !matches!(
                                        state,
                                        bollard::models::LocalNodeState::EMPTY
                                            | bollard::models::LocalNodeState::INACTIVE
                                    )
                                })
                            })
                            .unwrap_or(false);
                        host.swarm_active = swarm_active;

                        if swarm_active {
                            if let Ok(swarm) = docker.inspect_swarm().await {
                                host.swarm_id = swarm.id.clone();
                                host.swarm_name =
                                    swarm.spec.as_ref().and_then(|s| s.name.clone());
                            }
        if let Ok(nodes) = node::list_nodes(&docker).await {
                                host.node_count = Some(nodes.len() as u64);
                                host.manager_count = Some(
                                    nodes
                                        .iter()
                                        .filter(|n| {
                                            n.spec
                                                .as_ref()
                                                .and_then(|s| s.role)
                                                .map(|r| {
                                                    r == bollard::models::NodeSpecRoleEnum::MANAGER
                                                })
                                                .unwrap_or(false)
                                        })
                                        .count() as u64,
                                );
                            }
                        }
                        result = Some(host);
                    }
                }
            }

            result
        });
        futures.push(fut);

        if futures.len() >= MAX_CONCURRENT_SCANS {
            let done: Vec<_> = futures.drain(..).collect();
            for f in done {
                if let Ok(Some(host)) = f.await {
                    discovered.push(host);
                }
            }
        }
    }

    for f in futures {
        if let Ok(Some(host)) = f.await {
            discovered.push(host);
        }
    }

    discovered.sort_by(|a, b| {
        let a_score = if a.swarm_active { 2 } else if a.docker_port.is_some() { 1 } else { 0 };
        let b_score = if b.swarm_active { 2 } else if b.docker_port.is_some() { 1 } else { 0 };
        b_score.cmp(&a_score).then_with(|| a.ip.cmp(&b.ip))
    });

    log::info!("Scan complete: {}/{} hosts responded", discovered.len(), total);

    Ok(discovered)
}
