use serde::Serialize;
use tabled::Tabled;

/// Reachability of an endpoint, as observed by a health probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EndpointStatus {
    /// Responded, and the node is a swarm manager.
    Healthy,
    /// Responded, but the daemon is not in a swarm.
    NoSwarm,
    /// Could not connect, or the probe timed out.
    Unreachable,
    /// Responded with an error that is not "not a manager".
    Error,
}

impl EndpointStatus {
    pub fn label(&self) -> &'static str {
        match self {
            EndpointStatus::Healthy => "ok",
            EndpointStatus::NoSwarm => "no-swarm",
            EndpointStatus::Unreachable => "unreachable",
            EndpointStatus::Error => "error",
        }
    }
}

impl std::fmt::Display for EndpointStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// One row of `swarmctl endpoint health`.
#[derive(Debug, Clone, Tabled, Serialize)]
pub struct EndpointHealthRow {
    pub name: String,
    pub environment: String,
    pub site: String,
    pub status: String,
    pub host: String,
    pub swarm: String,
    pub nodes: String,
    pub version: String,
    pub rtt_ms: String,
    pub error: String,
}

/// Placeholder for an absent value in a table cell.
pub const NONE_CELL: &str = "-";

/// One row of `swarmctl endpoint ls`.
#[derive(Debug, Clone, Tabled, Serialize)]
pub struct EndpointRow {
    pub name: String,
    pub host: String,
    pub environment: String,
    pub site: String,
    pub tags: String,
    pub current: bool,
    pub description: String,
}

/// Health of a single endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct EndpointHealth {
    pub name: String,
    pub host: String,
    pub status: EndpointStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swarm_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swarm_name: Option<String>,
    pub node_count: Option<u64>,
    pub manager_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    /// Round-trip time of the `/_ping` call, in milliseconds.
    pub rtt_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl EndpointHealth {
    /// Render as a table row.
    pub fn to_row(&self, environment: Option<String>, site: Option<String>) -> EndpointHealthRow {
        EndpointHealthRow {
            name: self.name.clone(),
            environment: environment.unwrap_or_else(|| NONE_CELL.into()),
            site: site.unwrap_or_else(|| NONE_CELL.into()),
            status: self.status.label().to_string(),
            host: self.host.clone(),
            swarm: match (&self.swarm_name, &self.swarm_id) {
                (Some(n), Some(id)) => format!("{} ({})", n, &id[..id.len().min(12)]),
                (Some(n), None) => n.clone(),
                (None, Some(id)) => id[..id.len().min(12)].to_string(),
                (None, None) => NONE_CELL.into(),
            },
            nodes: match (self.node_count, self.manager_count) {
                (Some(n), Some(m)) => format!("{} ({} mgr)", n, m),
                (Some(n), None) => n.to_string(),
                _ => NONE_CELL.into(),
            },
            version: self
                .docker_version
                .clone()
                .unwrap_or_else(|| NONE_CELL.into()),
            rtt_ms: self
                .rtt_ms
                .map(|r| r.to_string())
                .unwrap_or_else(|| NONE_CELL.into()),
            error: self
                .error
                .clone()
                .filter(|e| !e.is_empty())
                .unwrap_or_else(|| NONE_CELL.into()),
        }
    }

    /// Whether the endpoint answered at all.
    pub fn reachable(&self) -> bool {
        !matches!(
            self.status,
            EndpointStatus::Unreachable | EndpointStatus::Error
        )
    }
}

/// A single field-level difference between two clusters.
#[derive(Debug, Clone, Serialize)]
pub struct FieldDiff {
    pub field: String,
    pub left: String,
    pub right: String,
}

/// How one resource differs between two endpoints.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceDiff {
    pub kind: String,
    pub name: String,
    /// `added`, `removed`, or `changed`.
    pub change: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldDiff>,
}

/// Aggregate comparison of two endpoints.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterDiff {
    pub left: String,
    pub right: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_unreachable: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right_unreachable: Option<String>,
    pub identical: bool,
    pub diffs: Vec<ResourceDiff>,
}

impl Default for ClusterDiff {
    fn default() -> Self {
        Self {
            left: String::new(),
            right: String::new(),
            left_unreachable: None,
            right_unreachable: None,
            identical: true,
            diffs: Vec::new(),
        }
    }
}

impl ClusterDiff {
    pub fn diff_count(&self) -> usize {
        self.diffs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health(status: EndpointStatus) -> EndpointHealth {
        EndpointHealth {
            name: "prod-eu".into(),
            host: "ssh://ops@10.8.0.4".into(),
            status,
            swarm_id: Some("abcdef0123456789".into()),
            swarm_name: Some("prod".into()),
            node_count: Some(12),
            manager_count: Some(3),
            docker_version: Some("29.7.2".into()),
            hostname: Some("manager-1".into()),
            rtt_ms: Some(34),
            error: None,
        }
    }

    #[test]
    fn status_labels_are_distinct() {
        let labels: Vec<&str> = [
            EndpointStatus::Healthy,
            EndpointStatus::NoSwarm,
            EndpointStatus::Unreachable,
            EndpointStatus::Error,
        ]
        .iter()
        .map(|s| s.label())
        .collect();
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), labels.len());
    }

    #[test]
    fn reachable_is_false_for_transport_failures() {
        assert!(health(EndpointStatus::Healthy).reachable());
        assert!(health(EndpointStatus::NoSwarm).reachable());
        assert!(!health(EndpointStatus::Unreachable).reachable());
        assert!(!health(EndpointStatus::Error).reachable());
    }

    #[test]
    fn row_summarises_nodes_and_managers() {
        let row = health(EndpointStatus::Healthy).to_row(Some("prod".into()), Some("eu".into()));
        assert_eq!(row.nodes, "12 (3 mgr)");
        assert_eq!(row.status, "ok");
        assert_eq!(row.rtt_ms, "34");
        // Swarm name plus a short id.
        assert!(row.swarm.starts_with("prod (abcdef012345"));
    }

    #[test]
    fn row_handles_missing_swarm_and_counts() {
        let mut h = health(EndpointStatus::NoSwarm);
        h.swarm_id = None;
        h.swarm_name = None;
        h.node_count = None;
        h.manager_count = None;
        let row = h.to_row(None, None);
        assert_eq!(row.swarm, NONE_CELL);
        assert_eq!(row.nodes, NONE_CELL);
        assert_eq!(row.environment, NONE_CELL);
    }

    #[test]
    fn short_swarm_id_does_not_panic_on_a_short_id() {
        let mut h = health(EndpointStatus::Healthy);
        h.swarm_id = Some("ab".into());
        h.swarm_name = None;
        let row = h.to_row(None, None);
        assert_eq!(row.swarm, "ab");
    }

    #[test]
    fn diff_count_matches_diffs() {
        let d = ClusterDiff {
            left: "a".into(),
            right: "b".into(),
            left_unreachable: None,
            right_unreachable: None,
            identical: false,
            diffs: vec![
                ResourceDiff {
                    kind: "service".into(),
                    name: "web".into(),
                    change: "changed".into(),
                    fields: vec![FieldDiff {
                        field: "replicas".into(),
                        left: "1".into(),
                        right: "3".into(),
                    }],
                },
                ResourceDiff {
                    kind: "service".into(),
                    name: "api".into(),
                    change: "added".into(),
                    fields: vec![],
                },
            ],
        };
        assert_eq!(d.diff_count(), 2);
        assert!(!d.identical, "a diff list means not identical");
    }
}
