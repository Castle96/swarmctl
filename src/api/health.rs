//! Health probing for registered endpoints.
//!
//! One probe per endpoint, in parallel, each with its own timeout, so a single
//! unreachable cluster cannot stall or fail the others.

use crate::api::client::DockerClient;
use crate::api::endpoints::{Endpoint, to_connection_config};
use crate::models::endpoint::{EndpointHealth, EndpointStatus};
use std::time::{Duration, Instant};

/// How long a single endpoint gets to answer.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Probe several endpoints concurrently.
pub async fn probe_all(endpoints: &[Endpoint]) -> Vec<EndpointHealth> {
    let mut handles = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints {
        handles.push(tokio::spawn(probe(endpoint.clone())));
    }

    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        match handle.await {
            Ok(health) => results.push(health),
            Err(e) => results.push(EndpointHealth {
                name: "<unknown>".to_string(),
                host: "-".to_string(),
                status: EndpointStatus::Error,
                swarm_id: None,
                swarm_name: None,
                node_count: None,
                manager_count: None,
                docker_version: None,
                hostname: None,
                rtt_ms: None,
                error: Some(format!("probe task failed: {}", e)),
            }),
        }
    }

    // Preserve the caller's ordering rather than completion order.
    let mut ordered = Vec::with_capacity(results.len());
    for endpoint in endpoints {
        if let Some(pos) = results.iter().position(|h| h.name == endpoint.name) {
            ordered.push(results.remove(pos));
        }
    }
    ordered
}

/// Probe one endpoint: connect, then read daemon, swarm and node state.
pub async fn probe(endpoint: Endpoint) -> EndpointHealth {
    let mut health = EndpointHealth {
        name: endpoint.name.clone(),
        host: endpoint.host.clone(),
        status: EndpointStatus::Unreachable,
        swarm_id: None,
        swarm_name: None,
        node_count: None,
        manager_count: None,
        docker_version: None,
        hostname: None,
        rtt_ms: None,
        error: None,
    };

    // Surface a configuration mistake as such, rather than as a connect failure.
    if let Err(e) = crate::api::endpoints::validate(&endpoint) {
        health.error = Some(e.to_string());
        return health;
    }

    let config = to_connection_config(&endpoint);
    let client = match DockerClient::with_config(&config) {
        Ok(c) => c,
        Err(e) => {
            health.error = Some(e.to_string());
            return health;
        }
    };

    let started = Instant::now();

    // `/_ping` is the cheapest call that proves the daemon is answering, and
    // gives a clean round-trip time for the health table.
    let ping = tokio::time::timeout(PROBE_TIMEOUT, client.inner().ping()).await;

    match ping {
        Err(_) => {
            health.error = Some(format!("timed out after {}s", PROBE_TIMEOUT.as_secs()));
            return health;
        }
        Ok(Err(e)) => {
            health.error = Some(e.to_string());
            return health;
        }
        Ok(Ok(_)) => {
            // bollard's ping resolves to the daemon's status string.
            health.rtt_ms = Some(started.elapsed().as_millis() as u64);
        }
    }

    let info = match tokio::time::timeout(PROBE_TIMEOUT, client.inner().info()).await {
        Ok(Ok(info)) => info,
        Ok(Err(e)) => {
            health.status = EndpointStatus::Error;
            health.error = Some(e.to_string());
            return health;
        }
        Err(_) => {
            health.error = Some("timed out reading daemon info".to_string());
            return health;
        }
    };

    health.docker_version = info.server_version.clone();
    health.hostname = info.name.clone();

    let swarm_state = info.swarm.as_ref().and_then(|s| s.local_node_state);
    let swarm_active = swarm_state.as_ref().is_some_and(|s| {
        !matches!(
            s,
            bollard::models::LocalNodeState::EMPTY | bollard::models::LocalNodeState::INACTIVE
        )
    });

    if !swarm_active {
        health.status = EndpointStatus::NoSwarm;
        return health;
    }

    // Swarm identity and size. A non-manager cannot inspect the swarm, so those
    // stay empty rather than being reported as an error.
    if let Ok(swarm) = tokio::time::timeout(PROBE_TIMEOUT, client.inner().inspect_swarm()).await
        && let Ok(swarm) = swarm
    {
        health.swarm_id = swarm.id.clone();
        health.swarm_name = swarm.spec.as_ref().and_then(|s| s.name.clone());
    }

    match tokio::time::timeout(PROBE_TIMEOUT, crate::api::node::list_nodes(client.inner())).await {
        Ok(Ok(nodes)) => {
            health.node_count = Some(nodes.len() as u64);
            health.manager_count = Some(
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
        }
        Ok(Err(e)) => {
            health.status = EndpointStatus::Error;
            health.error = Some(e.to_string());
            return health;
        }
        Err(_) => {
            health.error = Some("timed out listing nodes".to_string());
        }
    }

    health.status = EndpointStatus::Healthy;
    health
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_timeout_is_bounded() {
        // An unreachable offsite host must not hang the whole health command.
        assert!(PROBE_TIMEOUT <= Duration::from_secs(10));
    }

    #[tokio::test]
    async fn probe_of_unreachable_host_reports_rather_than_hanging() {
        // Port 1 on loopback refuses connections.
        let endpoint = Endpoint {
            name: "dead".into(),
            host: "tcp://127.0.0.1:1".into(),
            description: None,
            site: None,
            environment: None,
            tags: Vec::new(),
            tls: None,
            default: false,
        };

        let health = tokio::time::timeout(Duration::from_secs(20), probe(endpoint))
            .await
            .expect("probe must not hang");

        assert!(!health.reachable());
        assert_eq!(health.status, EndpointStatus::Unreachable);
        assert!(health.error.is_some(), "expected an error message");
    }

    #[tokio::test]
    async fn probe_of_invalid_scheme_is_reported() {
        let endpoint = Endpoint {
            name: "bad".into(),
            host: "not-a-scheme".into(),
            description: None,
            site: None,
            environment: None,
            tags: Vec::new(),
            tls: None,
            default: false,
        };
        let health = probe(endpoint).await;
        assert!(!health.reachable());
        assert!(health.error.unwrap().contains("must start with"));
    }

    #[tokio::test]
    async fn probe_all_isolates_failures() {
        let make = |name: &str, host: &str| Endpoint {
            name: name.into(),
            host: host.into(),
            description: None,
            site: None,
            environment: None,
            tags: Vec::new(),
            tls: None,
            default: false,
        };
        let endpoints = vec![
            make("a", "tcp://127.0.0.1:1"),
            make("b", "tcp://127.0.0.1:2"),
            make("c", "tcp://127.0.0.1:3"),
        ];

        let results = tokio::time::timeout(Duration::from_secs(30), probe_all(&endpoints))
            .await
            .expect("probe_all must not hang");

        assert_eq!(results.len(), 3, "one result per endpoint");
        // Order matches the input, regardless of which finished first.
        let names: Vec<&str> = results.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn probe_all_with_no_endpoints_is_empty() {
        assert!(probe_all(&[]).await.is_empty());
    }
}
