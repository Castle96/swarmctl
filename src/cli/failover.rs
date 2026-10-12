use crate::api::client::DockerClient;
use crate::cli::root::OutputFormat;
use crate::models::failover::FailoverEvent;
use anyhow::Context;
use tokio::sync::mpsc;

pub struct FailoverManager {
    monitor: crate::api::failover::FailoverMonitor,
    event_rx: mpsc::UnboundedReceiver<FailoverEvent>,
    #[allow(dead_code)]
    event_tx: mpsc::UnboundedSender<FailoverEvent>,
    /// Accumulated events.
    ///
    /// A plain `Vec`, not `Arc<Mutex<_>>`. `check` takes `&mut self`, so the
    /// `Arc` was never needed — and this field had already caused a
    /// self-deadlock when the guard was held across the drain loop below.
    events: Vec<FailoverEvent>,
}

impl FailoverManager {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let monitor = crate::api::failover::FailoverMonitor::new(event_tx.clone());

        Self {
            monitor,
            event_rx,
            event_tx,
            events: Vec::new(),
        }
    }

    pub async fn check(&mut self, client: &DockerClient) -> anyhow::Result<()> {
        let nodes = client.inner().list_nodes(None).await?;
        let tasks = client.inner().list_tasks(None).await?;

        let events = self.monitor.check_and_update(&nodes, &tasks);

        // No lock needed: `check` holds `&mut self`. The previous version held
        // a `MutexGuard` from the enclosing scope and then re-locked the same
        // non-reentrant mutex inside the `try_recv` loop, which deadlocked as
        // soon as the loop body ran — i.e. on the second observation.
        self.events.extend(events);

        while let Ok(event) = self.event_rx.try_recv() {
            self.events.push(event);
        }

        Ok(())
    }

    /// Reflect the node list as it stands right now.
    pub async fn snapshot(&mut self, client: &DockerClient) -> anyhow::Result<()> {
        let nodes = client.inner().list_nodes(None).await?;
        self.monitor.observe_current(&nodes);
        Ok(())
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.monitor.set_enabled(enabled);
    }

    pub fn get_state(&self) -> crate::models::failover::FailoverState {
        self.monitor.get_state()
    }

    pub fn get_events(&self) -> Vec<FailoverEvent> {
        self.events.clone()
    }
}

/// How long `failover status` watches for changes before reporting.
const OBSERVE_SECONDS: u64 = 6;
const OBSERVE_INTERVAL_SECONDS: u64 = 2;

pub async fn run_status(client: &DockerClient, _output: OutputFormat) -> anyhow::Result<()> {
    let enabled = load_enabled();

    let mut manager = FailoverManager::new();
    manager.set_enabled(enabled);

    // Baseline sample, so later observations can detect transitions...
    manager.check(client).await?;
    // ...and a direct snapshot, so a node that is down *now* is reported even
    // though no transition has been observed yet.
    manager.snapshot(client).await?;

    eprintln!("Observing for {}s...", OBSERVE_SECONDS);
    let mut elapsed = 0;
    while elapsed < OBSERVE_SECONDS {
        tokio::time::sleep(std::time::Duration::from_secs(OBSERVE_INTERVAL_SECONDS)).await;
        elapsed += OBSERVE_INTERVAL_SECONDS;
        manager.check(client).await?;
    }
    manager.snapshot(client).await?;

    let state = manager.get_state();
    let events = manager.get_events();

    println!();
    println!("Failover Status");
    println!("═══════════════════════════════════════");
    println!(
        "  Enabled:      {}",
        if state.enabled { "Yes" } else { "No" }
    );
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
                FailoverEvent::NodeDown {
                    node_name,
                    timestamp,
                    ..
                } => {
                    println!("  [{}] NODE_DOWN: {}", timestamp, node_name);
                }
                FailoverEvent::TaskRescheduled {
                    service_name,
                    to_node,
                    timestamp,
                    ..
                } => {
                    println!(
                        "  [{}] TASK_RESCHEDULED: {} -> {}",
                        timestamp, service_name, to_node
                    );
                }
                FailoverEvent::DataMigrationStarted {
                    volume_name,
                    to_node,
                    timestamp,
                    ..
                } => {
                    println!(
                        "  [{}] MIGRATION_STARTED: {} -> {}",
                        timestamp, volume_name, to_node
                    );
                }
                FailoverEvent::DataMigrationCompleted {
                    volume_name,
                    bytes_copied,
                    duration_ms,
                    timestamp,
                    ..
                } => {
                    println!(
                        "  [{}] MIGRATION_COMPLETED: {} ({} bytes, {}ms)",
                        timestamp, volume_name, bytes_copied, duration_ms
                    );
                }
                FailoverEvent::DataMigrationFailed {
                    volume_name,
                    error,
                    timestamp,
                    ..
                } => {
                    println!(
                        "  [{}] MIGRATION_FAILED: {} - {}",
                        timestamp, volume_name, error
                    );
                }
                FailoverEvent::ContainerRedeployed {
                    service_name,
                    target_node,
                    timestamp,
                    ..
                } => {
                    println!(
                        "  [{}] CONTAINER_REDEPLOYED: {} -> {}",
                        timestamp, service_name, target_node
                    );
                }
            }
        }
    }

    println!();
    Ok(())
}

fn state_file() -> std::path::PathBuf {
    crate::utils::paths::failover_file()
}

fn load_enabled() -> bool {
    std::fs::read_to_string(state_file())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("enabled").and_then(|e| e.as_bool()))
        .unwrap_or(false)
}

fn save_enabled(enabled: bool) -> anyhow::Result<()> {
    let path = state_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::json!({ "enabled": enabled }).to_string())
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub async fn run_enable(_client: &DockerClient) -> anyhow::Result<()> {
    // Previously this flipped a flag on a manager that was immediately dropped,
    // so nothing was persisted and `status` never showed it.
    save_enabled(true)?;
    println!("Failover event monitoring enabled.");
    println!("  `swarmctl failover status` will now report node-down and redeploy events.");
    println!("  Note: swarmctl does not reschedule containers automatically; Docker's own");
    println!("  scheduler handles that. This flag only controls event reporting.");
    Ok(())
}

pub async fn run_disable(_client: &DockerClient) -> anyhow::Result<()> {
    save_enabled(false)?;
    println!("Failover event monitoring disabled.");
    Ok(())
}

pub async fn run_migrate(
    _client: &DockerClient,
    volume: String,
    from_node: String,
    to_node: String,
    _output: OutputFormat,
) -> anyhow::Result<()> {
    // This used to pass the *same* Docker client as both source and target, so no
    // data ever crossed nodes, and then printed "Migration completed" and exited
    // 0. The Docker API has no endpoint for copying a volume between nodes: it
    // would require filesystem access to each node's local daemon. Refuse rather
    // than claim success.
    anyhow::bail!(
        "Cannot migrate volume '{}' from '{}' to '{}': moving volume data between nodes \
         is not implemented.\n\
         \n\
         The Docker Engine API exposes volumes only on the local daemon, so this needs \
         filesystem access to both nodes (for example over SSH), which swarmctl does \
         not have.\n\
         \n\
         Alternatives: recreate the service with a fresh volume and copy the data \
         yourself, or use a networked filesystem (NFS, Ceph, EFS) shared by all nodes.",
        volume,
        from_node,
        to_node
    )
}
