use crate::api::client::DockerClient;
use crate::api::swarm;
use anyhow::Result;

pub async fn run_init(client: &DockerClient, advertise_addr: Option<String>) -> Result<()> {
    if swarm::is_swarm_active(client.inner()).await {
        println!("Swarm is already active on this node.");
        let info = swarm::get_swarm_info(client.inner()).await?;
        if let Some(spec) = &info.spec {
            println!(
                "  Cluster: {}",
                spec.name.as_deref().unwrap_or("unnamed")
            );
        }
        println!(
            "  ID: {}",
            info.id.as_deref().unwrap_or("unknown")
        );
        return Ok(());
    }

    let addr = match advertise_addr {
        Some(a) => a,
        None => detect_local_ip(),
    };

    println!("Initializing Docker swarm...");
    println!("  Advertise address: {}", addr);

    let node_id = swarm::init_swarm(client.inner(), &addr).await?;
    println!("  Node ID: {}", node_id);

    let tokens = swarm::get_join_tokens(client.inner()).await?;
    println!();
    println!("Swarm initialized successfully.");
    println!();
    println!("  Worker join token:  {}...", &tokens.worker[..24.min(tokens.worker.len())]);
    println!("  Manager join token: {}...", &tokens.manager[..24.min(tokens.manager.len())]);

    let vault_password = rpassword::prompt_password("Vault password (to save tokens): ")?;
    if vault_password.is_empty() {
        println!("Tokens not saved — no vault password provided.");
        return Ok(());
    }

    let vault = if crate::vault::LocalVault::exists() {
        crate::vault::LocalVault::open(&vault_password)?
    } else {
        crate::vault::LocalVault::create(&vault_password)?
    };

    let mut vault = vault;
    let host = std::env::var("DOCKER_HOST").unwrap_or_else(|_| "unix:///var/run/docker.sock".to_string());
    let swarm_name = info_name(client).await;
    vault.store_swarm_tokens(tokens, None, &host, &swarm_name)?;

    println!();
    println!("Join tokens saved to vault.");

    Ok(())
}

pub async fn run_join(
    client: &DockerClient,
    token: String,
    remote_addr: String,
    advertise_addr: Option<String>,
) -> Result<()> {
    if swarm::is_swarm_active(client.inner()).await {
        println!("This node is already part of a swarm.");
        return Ok(());
    }

    let addr = match advertise_addr {
        Some(a) => a,
        None => detect_local_ip(),
    };

    println!("Joining swarm at {}...", remote_addr);
    println!("  Advertise address: {}", addr);

    swarm::join_swarm(client.inner(), &addr, &remote_addr, &token).await?;

    println!("Successfully joined the swarm.");

    Ok(())
}

pub async fn run_leave(client: &DockerClient, force: bool) -> Result<()> {
    if !swarm::is_swarm_active(client.inner()).await {
        println!("This node is not part of a swarm.");
        return Ok(());
    }

    if force {
        println!("Force leaving swarm...");
    } else {
        println!("Leaving swarm...");
    }

    swarm::leave_swarm(client.inner(), force).await?;
    println!("Left the swarm.");

    Ok(())
}

pub async fn run_token(
    client: &DockerClient,
    worker: bool,
    manager: bool,
    rotate: bool,
) -> Result<()> {
    if !swarm::is_swarm_active(client.inner()).await {
        println!("No active swarm on this node.");
        return Ok(());
    }

    let tokens = if rotate {
        println!("Rotating join tokens...");
        let new_tokens = swarm::rotate_join_tokens(client.inner()).await?;
        println!("New tokens generated.");

        if crate::vault::LocalVault::exists() {
            let vault_password = rpassword::prompt_password("Vault password (to update tokens): ")?;
            if !vault_password.is_empty() {
                let vault = crate::vault::LocalVault::open(&vault_password);
                match vault {
                    Ok(mut v) => {
                        v.rotate_tokens(new_tokens.clone())?;
                        println!("Tokens updated in vault.");
                    }
                    Err(e) => {
                        println!("Warning: could not update vault: {}", e);
                    }
                }
            }
        }

        new_tokens
    } else {
        swarm::get_join_tokens(client.inner()).await?
    };

    if worker || (!worker && !manager) {
        println!();
        println!("Worker token:  {}", tokens.worker);
    }
    if manager || (!worker && !manager) {
        println!();
        println!("Manager token: {}", tokens.manager);
    }

    Ok(())
}

pub async fn run_status(client: &DockerClient) -> Result<()> {
    if !swarm::is_swarm_active(client.inner()).await {
        println!("No active swarm on this node.");
        return Ok(());
    }

    let info = swarm::get_swarm_info(client.inner()).await?;

    println!();
    println!("Swarm Status");
    println!("═══════════════════════════════════════");

    if let Some(spec) = &info.spec {
        println!(
            "  Name:       {}",
            spec.name.as_deref().unwrap_or("unnamed")
        );
    }
    println!(
        "  ID:         {}",
        info.id.as_deref().unwrap_or("unknown")
    );
    println!(
        "  Created:    {}",
        info.created_at
            .as_ref()
            .map(|d| format!("{}", d))
            .unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "  Updated:    {}",
        info.updated_at
            .as_ref()
            .map(|d| format!("{}", d))
            .unwrap_or_else(|| "unknown".to_string())
    );

    let managers = crate::api::node::get_managers(client.inner()).await?;
    let workers = crate::api::node::get_workers(client.inner()).await?;

    println!();
    println!("  Managers:   {}", managers.len());
    for m in &managers {
        let name = m
            .spec
            .as_ref()
            .and_then(|s| s.name.as_deref())
            .unwrap_or("unknown");
        let status = m
            .status
            .as_ref()
            .and_then(|s| s.state)
            .map(|s| format!("{:?}", s))
            .unwrap_or_else(|| "unknown".to_string());
        println!("    - {} ({})", name, status);
    }

    println!();
    println!("  Workers:    {}", workers.len());
    for w in &workers {
        let name = w
            .spec
            .as_ref()
            .and_then(|s| s.name.as_deref())
            .unwrap_or("unknown");
        let status = w
            .status
            .as_ref()
            .and_then(|s| s.state)
            .map(|s| format!("{:?}", s))
            .unwrap_or_else(|| "unknown".to_string());
        println!("    - {} ({})", name, status);
    }

    if let Some(raft) = info.spec.as_ref().and_then(|s| s.raft.as_ref()) {
        println!();
        println!("  Raft Config:");
        if let Some(tick) = raft.heartbeat_tick {
            println!("    Heartbeat tick: {}", tick);
        }
        if let Some(tick) = raft.election_tick {
            println!("    Election tick:  {}", tick);
        }
    }

    println!();
    Ok(())
}

async fn info_name(client: &DockerClient) -> String {
    client
        .inner()
        .info()
        .await
        .ok()
        .and_then(|i| i.name)
        .unwrap_or_else(|| "docker".to_string())
}

fn detect_local_ip() -> String {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok();
    socket
        .and_then(|s| {
            s.connect("8.8.8.8:80").ok()?;
            s.local_addr().ok()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string())
}
