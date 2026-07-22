use crate::api::client::DockerClient;
use crate::cli::root::OutputFormat;
use crate::models::failover::FailoverEvent;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct FailoverManager {
    monitor: crate::api::failover::FailoverMonitor,
    event_rx: mpsc::UnboundedReceiver<FailoverEvent>,
    #[allow(dead_code)]
    event_tx: mpsc::UnboundedSender<FailoverEvent>,
    events: Arc<Mutex<Vec<FailoverEvent>>>,
}

impl FailoverManager {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let events = Arc::new(Mutex::new(Vec::new()));
        let monitor = crate::api::failover::FailoverMonitor::new(event_tx.clone());

        Self {
            monitor,
            event_rx,
            event_tx,
            events,
        }
    }

    pub async fn check(&mut self, client: &DockerClient) -> anyhow::Result<()> {
        let nodes = client.inner().list_nodes(None).await?;
        let tasks = client.inner().list_tasks(None).await?;

        let events = self.monitor.check_and_update(&nodes, &tasks);

        let mut stored_events = self.events.lock().unwrap();
        for event in events {
            stored_events.push(event);
        }

        while let Ok(event) = self.event_rx.try_recv() {
            let mut stored_events = self.events.lock().unwrap();
            stored_events.push(event);
        }

        Ok(())
    }

    pub fn get_state(&self) -> crate::models::failover::FailoverState {
        self.monitor.get_state()
    }

    pub fn get_events(&self) -> Vec<FailoverEvent> {
        self.events.lock().unwrap().clone()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.monitor.set_enabled(enabled);
    }
}

pub async fn run_status(
    client: &DockerClient,
    _output: OutputFormat,
) -> anyhow::Result<()> {
    let mut manager = FailoverManager::new();
    manager.check(client).await?;

    let state = manager.get_state();
    let events = manager.get_events();

    println!();
    println!("Failover Status");
    println!("═══════════════════════════════════════");
    println!("  Enabled:      {}", if state.enabled { "Yes" } else { "No" });
    println!("  Failed nodes: {}", state.failed_nodes.len());
    for node in &state.failed_nodes {
        println!("    - {}", node);
    }
    println!("  Migrations:   {}", state.migrations_in_progress.len());
    println!();

    if events.is_empty() {
        println!("  No failover events recorded.");
    } else {
        println!("  Recent Events:");
        println!("  ─────────────────────────────────────");
        for event in events.iter().rev().take(20) {
            match event {
                FailoverEvent::NodeDown { node_name, timestamp, .. } => {
                    println!("  [{}] NODE_DOWN: {}", timestamp, node_name);
                }
                FailoverEvent::TaskRescheduled { service_name, to_node, timestamp, .. } => {
                    println!("  [{}] TASK_RESCHEDULED: {} -> {}", timestamp, service_name, to_node);
                }
                FailoverEvent::DataMigrationStarted { volume_name, to_node, timestamp, .. } => {
                    println!("  [{}] MIGRATION_STARTED: {} -> {}", timestamp, volume_name, to_node);
                }
                FailoverEvent::DataMigrationCompleted { volume_name, bytes_copied, duration_ms, timestamp, .. } => {
                    println!("  [{}] MIGRATION_COMPLETED: {} ({} bytes, {}ms)", timestamp, volume_name, bytes_copied, duration_ms);
                }
                FailoverEvent::DataMigrationFailed { volume_name, error, timestamp, .. } => {
                    println!("  [{}] MIGRATION_FAILED: {} - {}", timestamp, volume_name, error);
                }
                FailoverEvent::ContainerRedeployed { service_name, target_node, timestamp, .. } => {
                    println!("  [{}] CONTAINER_REDEPLOYED: {} -> {}", timestamp, service_name, target_node);
                }
            }
        }
    }

    println!();
    Ok(())
}

pub async fn run_enable(_client: &DockerClient) -> anyhow::Result<()> {
    let manager = FailoverManager::new();
    manager.set_enabled(true);
    println!("Failover monitoring enabled.");
    println!("  The system will now monitor for node failures and automatically reschedule containers.");
    Ok(())
}

pub async fn run_disable(_client: &DockerClient) -> anyhow::Result<()> {
    let manager = FailoverManager::new();
    manager.set_enabled(false);
    println!("Failover monitoring disabled.");
    println!("  Automatic failover and data migration will not occur.");
    Ok(())
}

pub async fn run_migrate(
    client: &DockerClient,
    volume: String,
    from_node: String,
    to_node: String,
    _output: OutputFormat,
) -> anyhow::Result<()> {
    println!("Migrating volume '{}' from {} to {}...", volume, from_node, to_node);

    let migration = crate::api::migration::migrate_volume(
        client.inner(),
        client.inner(),
        &volume,
        &from_node,
        &to_node,
    )
    .await?;

    println!();
    println!("Migration completed:");
    println!("  Volume:    {}", migration.volume_name);
    println!("  From:      {}", migration.source_node);
    println!("  To:        {}", migration.target_node);
    println!("  Status:    {:?}", migration.status);
    println!("  Bytes:     {}", migration.bytes_copied);

    Ok(())
}
