use serde::Serialize;
use tabled::Tabled;

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub enum FailoverEvent {
    NodeDown {
        node_id: String,
        node_name: String,
        timestamp: String,
    },
    TaskRescheduled {
        task_id: String,
        service_name: String,
        from_node: String,
        to_node: String,
        timestamp: String,
    },
    DataMigrationStarted {
        volume_name: String,
        from_node: String,
        to_node: String,
        timestamp: String,
    },
    DataMigrationCompleted {
        volume_name: String,
        from_node: String,
        to_node: String,
        bytes_copied: u64,
        duration_ms: u64,
        timestamp: String,
    },
    DataMigrationFailed {
        volume_name: String,
        from_node: String,
        to_node: String,
        error: String,
        timestamp: String,
    },
    ContainerRedeployed {
        service_name: String,
        task_id: String,
        target_node: String,
        timestamp: String,
    },
}

#[allow(dead_code)]
#[derive(Tabled, Serialize)]
pub struct FailoverEventRow {
    pub time: String,
    pub event_type: String,
    pub source: String,
    pub target: String,
    pub details: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FailoverState {
    /// Opt-in. Nothing acts on this flag automatically, so defaulting to true
    /// made `failover disable` meaningless and `status` misleading.
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub events: Vec<FailoverEvent>,
    #[serde(default)]
    pub failed_nodes: Vec<String>,
    #[serde(default)]
    pub migrations_in_progress: Vec<String>,
}
