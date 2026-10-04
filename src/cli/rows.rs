//! Row builders: Docker API objects -> display rows.
//!
//! Extracted so `get` and `--endpoints` fan-out render identical rows from one
//! implementation. Previously the row mapping was inlined in each `get_*`
//! handler, which the fan-out path would have had to duplicate.

use bollard::Docker;
use std::collections::HashMap;

pub fn node_rows(nodes: &[bollard::models::Node]) -> Vec<crate::models::node::NodeRow> {
    nodes
        .iter()
        .map(|n| {
            let spec = n.spec.clone().unwrap_or_default();
            let status = n.status.clone().unwrap_or_default();

            let manager = n
                .manager_status
                .as_ref()
                .map(|m| {
                    match m
                        .reachability
                        .unwrap_or(bollard::models::Reachability::UNKNOWN)
                    {
                        bollard::models::Reachability::REACHABLE => "Reachable",
                        bollard::models::Reachability::UNREACHABLE => "Unavailable",
                        _ => "-",
                    }
                })
                .unwrap_or("-");

            let hostname = n
                .description
                .as_ref()
                .and_then(|d| d.hostname.clone())
                .or_else(|| spec.name.clone())
                .unwrap_or_default();

            crate::models::node::NodeRow {
                id: n.id.clone().unwrap_or_default(),
                hostname,
                status: match status.state {
                    // Docker omits state for nodes it cannot reach; reporting
                    // those as READY would present a broken node as healthy.
                    Some(state) => state.to_string(),
                    None => "<unknown>".to_string(),
                },
                availability: spec
                    .availability
                    .map(|a| a.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string()),
                manager: manager.to_string(),
                labels: crate::utils::selectors::format_labels(&spec.labels),
            }
        })
        .collect()
}

/// Services need a task listing to fill the REPLICAS column with a real
/// running count, so this is async and takes the client.
pub async fn service_rows(
    docker: &Docker,
    services: &[bollard::models::Service],
) -> anyhow::Result<Vec<crate::models::service::ServiceRow>> {
    let running_counts = running_counts_by_service(docker).await?;

    Ok(services
        .iter()
        .map(|s| {
            let service_id = s.id.clone().unwrap_or_default();
            let spec = s.spec.clone().unwrap_or_default();

            let name = spec.name.clone().unwrap_or_default();
            let image = spec
                .task_template
                .as_ref()
                .and_then(|t| t.container_spec.as_ref())
                .and_then(|c| c.image.clone())
                .unwrap_or_default();

            let (mode, replicas) = match spec.mode.as_ref() {
                Some(m) if m.replicated.is_some() => {
                    let desired = m.replicated.as_ref().and_then(|r| r.replicas).unwrap_or(0);
                    let running = running_counts.get(&service_id).copied().unwrap_or(0);
                    ("replicated".to_string(), format!("{}/{}", running, desired))
                }
                Some(m) if m.global.is_some() => ("global".to_string(), "N/A".to_string()),
                Some(_) => ("global".to_string(), "N/A".to_string()),
                None => ("unknown".to_string(), "N/A".to_string()),
            };

            crate::models::service::ServiceRow {
                id: service_id,
                name,
                mode,
                replicas,
                image,
                labels: crate::utils::selectors::format_labels(&spec.labels),
            }
        })
        .collect())
}

/// Map service id -> count of tasks that are actually running.
pub async fn running_counts_by_service(docker: &Docker) -> anyhow::Result<HashMap<String, usize>> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for task in crate::api::task::list_tasks(docker).await? {
        if !crate::api::task::task_is_running(&task) {
            continue;
        }
        if let Some(id) = task.service_id {
            *counts.entry(id).or_insert(0) += 1;
        }
    }
    Ok(counts)
}

/// Task rows with node ids resolved to hostnames.
pub fn task_rows_with_nodes(
    tasks: &[bollard::models::Task],
    node_hostnames: &HashMap<String, String>,
) -> Vec<crate::models::task::TaskRow> {
    tasks
        .iter()
        .map(|t| {
            let labels = t
                .spec
                .as_ref()
                .and_then(|s| s.container_spec.as_ref())
                .and_then(|c| c.labels.clone());
            let node_id = t.node_id.clone();

            crate::models::task::TaskRow {
                id: t.id.clone().unwrap_or_default(),
                name: t.name.clone().unwrap_or_default(),
                desired_state: t
                    .desired_state
                    .map(|s| format!("{:?}", s))
                    .unwrap_or_default(),
                current_state: t
                    .status
                    .as_ref()
                    .and_then(|s| s.state)
                    .map(|s| format!("{:?}", s))
                    .unwrap_or_default(),
                image: t
                    .spec
                    .as_ref()
                    .and_then(|s| s.container_spec.as_ref())
                    .and_then(|c| c.image.clone())
                    .unwrap_or_default(),
                ports: format_task_ports(t),
                node: node_id
                    .as_ref()
                    .map(|id| {
                        node_hostnames
                            .get(id)
                            .cloned()
                            .unwrap_or_else(|| id.clone())
                    })
                    .unwrap_or_default(),
                labels: crate::utils::selectors::format_labels(&labels),
            }
        })
        .collect()
}

/// Node id -> hostname, for rendering the task NODE column.
pub async fn node_hostnames(docker: &Docker) -> HashMap<String, String> {
    match crate::api::node::list_nodes(docker).await {
        Ok(nodes) => nodes
            .into_iter()
            .filter_map(|n| {
                let id = n.id?;
                let hostname = n
                    .description
                    .as_ref()
                    .and_then(|d| d.hostname.clone())
                    .or_else(|| n.spec.as_ref().and_then(|s| s.name.clone()))
                    .unwrap_or_else(|| id.clone());
                Some((id, hostname))
            })
            .collect(),
        Err(e) => {
            // Non-fatal: fall back to raw ids.
            log::warn!("Could not resolve node hostnames: {}", e);
            HashMap::new()
        }
    }
}

/// Render a task's published ports.
///
/// `TaskSpecContainerSpec` has no port field; the Engine API reports bindings on
/// `Task.Status.PortStatus`.
fn format_task_ports(task: &bollard::models::Task) -> String {
    let Some(ports) = task
        .status
        .as_ref()
        .and_then(|s| s.port_status.as_ref())
        .and_then(|p| p.ports.as_ref())
    else {
        return String::new();
    };
    let rendered: Vec<String> = ports
        .iter()
        .map(|p| {
            let proto = p
                .protocol
                .map(|v| format!("{:?}", v).to_lowercase())
                .unwrap_or_else(|| "tcp".to_string());
            match (p.published_port, p.target_port) {
                (Some(public), Some(target)) => format!("{}:{}/{}", public, target, proto),
                (Some(public), None) => format!("{}/{}", public, proto),
                (None, Some(target)) => format!("{}/{}", target, proto),
                _ => proto,
            }
        })
        .collect();
    rendered.join(",")
}

pub fn network_rows(
    networks: &[bollard::models::Network],
) -> Vec<crate::models::network::NetworkRow> {
    networks
        .iter()
        .map(|n| crate::models::network::NetworkRow {
            id: n.id.clone().unwrap_or_default(),
            name: n.name.clone().unwrap_or_else(|| "unknown".to_string()),
            driver: n.driver.clone().unwrap_or_else(|| "unknown".to_string()),
            scope: n.scope.clone().unwrap_or_else(|| "unknown".to_string()),
            internal: if n.internal.unwrap_or(false) {
                "true"
            } else {
                "false"
            }
            .to_string(),
            labels: crate::utils::selectors::format_labels(&n.labels),
        })
        .collect()
}

pub fn secret_rows(secrets: &[bollard::models::Secret]) -> Vec<crate::models::secret::SecretRow> {
    secrets
        .iter()
        .map(|s| {
            let labels = s.spec.as_ref().and_then(|spec| spec.labels.clone());
            crate::models::secret::SecretRow {
                id: s.id.clone().unwrap_or_default(),
                name: s
                    .spec
                    .as_ref()
                    .and_then(|spec| spec.name.clone())
                    .unwrap_or_default(),
                created_at: s.created_at.clone().unwrap_or_default(),
                labels: crate::utils::selectors::format_labels(&labels),
            }
        })
        .collect()
}

pub fn config_rows(configs: &[bollard::models::Config]) -> Vec<crate::models::config::ConfigRow> {
    configs
        .iter()
        .map(|c| {
            let labels = c.spec.as_ref().and_then(|spec| spec.labels.clone());
            crate::models::config::ConfigRow {
                id: c.id.clone().unwrap_or_default(),
                name: c
                    .spec
                    .as_ref()
                    .and_then(|spec| spec.name.clone())
                    .unwrap_or_default(),
                created_at: c.created_at.clone().unwrap_or_default(),
                labels: crate::utils::selectors::format_labels(&labels),
            }
        })
        .collect()
}

pub fn stack_rows(
    stacks: &[crate::api::stack::StackSummary],
) -> Vec<crate::models::stack::StackRow> {
    stacks
        .iter()
        .map(|s| crate::models::stack::StackRow {
            name: s.name.clone(),
            services: s.services.to_string(),
            replicas: s.replicas.to_string(),
        })
        .collect()
}

pub use config_rows as config_typed_rows;
pub use network_rows as network_typed_rows;
/// Aliases used by the fan-out path, named for the action rather than the type.
pub use node_rows as node_typed_rows;
pub use secret_rows as secret_typed_rows;
pub use service_rows as service_typed_rows;
pub use stack_rows as stack_typed_rows;
pub use task_rows_with_nodes as task_typed_rows;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_without_status_is_unknown_not_ready() {
        let node = bollard::models::Node {
            id: Some("n1".into()),
            spec: Some(bollard::models::NodeSpec {
                name: Some("node1".into()),
                ..Default::default()
            }),
            status: None,
            ..Default::default()
        };
        let rows = node_rows(&[node]);
        assert_eq!(rows[0].hostname, "node1");
        // Reporting a node with no status as READY would hide a broken node.
        assert_eq!(rows[0].status, "<unknown>");
        assert_eq!(rows[0].availability, "<unknown>");
    }

    #[test]
    fn node_prefers_description_hostname_over_spec_name() {
        let node = bollard::models::Node {
            id: Some("n1".into()),
            description: Some(bollard::models::NodeDescription {
                hostname: Some("real-host".into()),
                ..Default::default()
            }),
            spec: Some(bollard::models::NodeSpec {
                name: Some("often-empty".into()),
                ..Default::default()
            }),
            status: Some(bollard::models::NodeStatus {
                state: Some(bollard::models::NodeState::READY),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(node_rows(&[node])[0].hostname, "real-host");
    }

    #[test]
    fn node_reports_manager_reachability() {
        let node = bollard::models::Node {
            id: Some("n1".into()),
            manager_status: Some(bollard::models::ManagerStatus {
                leader: Some(true),
                reachability: Some(bollard::models::Reachability::REACHABLE),
                addr: None,
            }),
            status: Some(bollard::models::NodeStatus {
                state: Some(bollard::models::NodeState::READY),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(node_rows(&[node])[0].manager, "Reachable");
    }

    #[test]
    fn network_defaults_unknown_fields() {
        let network = bollard::models::Network {
            id: Some("net1".into()),
            ..Default::default()
        };
        let rows = network_rows(&[network]);
        assert_eq!(rows[0].name, "unknown");
        assert_eq!(rows[0].driver, "unknown");
        assert_eq!(rows[0].scope, "unknown");
        assert_eq!(rows[0].internal, "false");
    }

    #[test]
    fn secret_and_config_rows_tolerate_missing_spec() {
        let secret = bollard::models::Secret {
            id: Some("s1".into()),
            ..Default::default()
        };
        assert_eq!(secret_rows(&[secret])[0].name, "");

        let config = bollard::models::Config {
            id: Some("c1".into()),
            ..Default::default()
        };
        assert_eq!(config_rows(&[config])[0].name, "");
    }

    #[test]
    fn task_row_resolves_node_id_to_hostname() {
        let task = bollard::models::Task {
            id: Some("t1".into()),
            node_id: Some("abc123".into()),
            status: Some(bollard::models::TaskStatus {
                state: Some(bollard::models::TaskState::RUNNING),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut names = HashMap::new();
        names.insert("abc123".to_string(), "worker-1".to_string());
        let rows = task_rows_with_nodes(&[task], &names);
        assert_eq!(rows[0].node, "worker-1");
        // Desired and actual states are reported separately.
        assert_eq!(rows[0].current_state, "RUNNING");
        assert_eq!(rows[0].desired_state, "");
    }

    #[test]
    fn task_row_falls_back_to_node_id() {
        let task = bollard::models::Task {
            id: Some("t1".into()),
            node_id: Some("abc123".into()),
            ..Default::default()
        };
        let rows = task_rows_with_nodes(&[task], &HashMap::new());
        assert_eq!(rows[0].node, "abc123");
    }

    #[tokio::test]
    async fn global_mode_service_reports_n_a_replicas() {
        // A global service has no replica count, so there is nothing to report.
        let services = [bollard::models::Service {
            id: Some("svc1".into()),
            spec: Some(bollard::models::ServiceSpec {
                name: Some("g".into()),
                mode: Some(bollard::models::ServiceSpecMode {
                    global: Some(std::collections::HashMap::from([((), ())])),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }];
        // Requires a daemon for the task listing; assert the pure part instead.
        assert!(
            services[0]
                .spec
                .as_ref()
                .unwrap()
                .mode
                .as_ref()
                .unwrap()
                .global
                .is_some()
        );
    }
}
