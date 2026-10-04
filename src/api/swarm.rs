use crate::vault::models::JoinTokens;
use bollard::Docker;
use bollard::models::{SwarmInitRequest, SwarmJoinRequest};
use bollard::query_parameters::{LeaveSwarmOptions, UpdateSwarmOptions};

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
    let options = LeaveSwarmOptions { force };
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

/// Rotate the cluster's join tokens.
///
/// `Spec.JoinTokens` is a read-only field in the Engine API: the daemon derives
/// tokens from the cluster CA key and ignores whatever a client writes there.
/// Rotation is requested with `rotate_worker_token` / `rotate_manager_token` on
/// the update call, and the new values are read back with `inspect_swarm` —
/// generating them client-side would hand out strings that cannot join.
pub async fn rotate_join_tokens(docker: &Docker) -> anyhow::Result<JoinTokens> {
    let swarm = docker.inspect_swarm().await?;
    let version = swarm
        .version
        .context("No swarm version")?
        .index
        .unwrap_or(0);
    // Pass the spec through unchanged: rotation is requested purely via query
    // parameters, and the spec must not be altered.
    let spec = swarm.spec.context("No swarm spec")?;

    let options = UpdateSwarmOptions {
        version: version as i64,
        rotate_worker_token: true,
        rotate_manager_token: true,
        ..Default::default()
    };
    docker
        .update_swarm(spec, options)
        .await
        .context("Failed to rotate join tokens")?;

    // Read back the tokens the daemon actually issued.
    get_join_tokens(docker).await
}

use anyhow::Context;
