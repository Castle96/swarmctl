use bollard::Docker;
use bollard::models::{SwarmInitRequest, SwarmJoinRequest};
use bollard::query_parameters::{LeaveSwarmOptions, UpdateSwarmOptions};
use crate::vault::models::JoinTokens;

pub async fn get_swarm_info(docker: &Docker) -> anyhow::Result<bollard::models::Swarm> {
    Ok(docker.inspect_swarm().await?)
}

pub async fn is_swarm_active(docker: &Docker) -> bool {
    docker.inspect_swarm().await.is_ok()
}

pub async fn init_swarm(docker: &Docker, advertise_addr: &str) -> anyhow::Result<String> {
    let config = SwarmInitRequest {
        advertise_addr: Some(advertise_addr.to_string()),
        listen_addr: Some("0.0.0.0:2377".to_string()),
        ..Default::default()
    };
    let node_id = docker.init_swarm(config).await?;
    Ok(node_id)
}

pub async fn join_swarm(
    docker: &Docker,
    advertise_addr: &str,
    remote_addr: &str,
    token: &str,
) -> anyhow::Result<()> {
    let config = SwarmJoinRequest {
        advertise_addr: Some(advertise_addr.to_string()),
        remote_addrs: Some(vec![remote_addr.to_string()]),
        join_token: Some(token.to_string()),
        ..Default::default()
    };
    docker.join_swarm(config).await?;
    Ok(())
}

pub async fn leave_swarm(docker: &Docker, force: bool) -> anyhow::Result<()> {
    let options = LeaveSwarmOptions {
        force,
    };
    docker.leave_swarm(Some(options)).await?;
    Ok(())
}

pub async fn get_join_tokens(docker: &Docker) -> anyhow::Result<JoinTokens> {
    let swarm = docker.inspect_swarm().await?;
    let tokens = swarm.join_tokens.context("No join tokens available")?;
    Ok(JoinTokens {
        worker: tokens.worker.unwrap_or_default(),
        manager: tokens.manager.unwrap_or_default(),
    })
}

pub async fn rotate_join_tokens(docker: &Docker) -> anyhow::Result<JoinTokens> {
    let mut swarm = docker.inspect_swarm().await?;
    let version = swarm
        .version
        .context("No swarm version")?
        .index
        .unwrap_or(0);
    let spec = swarm.spec.context("No swarm spec")?;

    let new_tokens = crate::vault::models::JoinTokens {
        worker: generate_token(),
        manager: generate_token(),
    };

    let tokens_container = swarm.join_tokens.get_or_insert_with(Default::default);
    tokens_container.worker = Some(new_tokens.worker.clone());
    tokens_container.manager = Some(new_tokens.manager.clone());

    let options = UpdateSwarmOptions {
        version: version as i64,
        ..Default::default()
    };
    docker.update_swarm(spec, options).await?;

    Ok(new_tokens)
}

pub async fn set_autolock(docker: &Docker, enabled: bool) -> anyhow::Result<()> {
    let swarm = docker.inspect_swarm().await?;
    let version = swarm
        .version
        .context("No swarm version")?
        .index
        .unwrap_or(0);
    let mut spec = swarm.spec.context("No swarm spec")?;

    let tokens = spec
        .encryption_config
        .get_or_insert_with(Default::default);
    tokens.auto_lock_managers = Some(enabled);

    let options = UpdateSwarmOptions {
        version: version as i64,
        ..Default::default()
    };
    docker.update_swarm(spec, options).await?;
    Ok(())
}

pub fn get_node_addr(docker: &Docker) -> anyhow::Result<String> {
    let info = futures::executor::block_on(docker.info())?;
    let addr = info
        .swarm
        .as_ref()
        .and_then(|s| s.node_addr.clone())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    Ok(addr)
}

fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.r#gen()).collect();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

use anyhow::Context;
