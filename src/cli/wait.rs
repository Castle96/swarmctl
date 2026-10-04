use crate::api::client::DockerClient;
use crate::cli::root::WaitResourceType;

/// How often the condition is re-evaluated.
const POLL_INTERVAL_SECONDS: u64 = 2;

/// Canonical, lowercased resource kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitTarget {
    Service,
    Task,
    Node,
}

impl WaitTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            WaitTarget::Service => "services",
            WaitTarget::Task => "tasks",
            WaitTarget::Node => "nodes",
        }
    }
}

/// Map a user-supplied resource string (or clap enum) onto a wait target.
///
/// Kept in sync with [`ResourceType`] by a test so the two cannot drift.
pub fn resolve_resource_type(s: &str) -> anyhow::Result<WaitTarget> {
    match s.to_lowercase().as_str() {
        "services" | "svc" | "service" => Ok(WaitTarget::Service),
        "tasks" | "po" | "task" => Ok(WaitTarget::Task),
        "nodes" | "no" | "node" => Ok(WaitTarget::Node),
        _ => Err(anyhow::anyhow!("Unknown resource type: {}", s)),
    }
}

/// Conditions each resource kind accepts.
///
/// Previously any unrecognised string fell through to an `Ok(true)` arm, so
/// `wait services web typo-condition` printed "Condition met" immediately.
fn validate_condition(target: WaitTarget, condition: &str) -> anyhow::Result<()> {
    let normalized = condition.to_lowercase();
    let allowed: &[&str] = match target {
        WaitTarget::Service => &[
            "available",
            "ready",
            "running",
            "exists",
            "deleted",
            "removed",
        ],
        WaitTarget::Task => &[
            "running",
            "complete",
            "completed",
            "exists",
            "deleted",
            "removed",
        ],
        WaitTarget::Node => &[
            "ready", "exists", "deleted", "removed", "down", "drained", "cordoned",
        ],
    };
    if allowed.contains(&normalized.as_str()) {
        Ok(())
    } else {
        anyhow::bail!(
            "Unknown condition '{}' for {}. Valid conditions: {}",
            condition,
            target.as_str(),
            allowed.join(", ")
        )
    }
}

pub async fn run(
    client: &DockerClient,
    resource: &str,
    name: String,
    condition: String,
    timeout_secs: u64,
) -> anyhow::Result<()> {
    let target = resolve_resource_type(resource)?;
    validate_condition(target, &condition)?;

    if timeout_secs == 0 {
        anyhow::bail!("--timeout must be greater than zero seconds");
    }

    let start = std::time::Instant::now();

    loop {
        let ready = match target {
            WaitTarget::Service => check_service_ready(client, &name, &condition).await?,
            WaitTarget::Task => check_task_ready(client, &name, &condition).await?,
            WaitTarget::Node => check_node_ready(client, &name, &condition).await?,
        };

        if ready {
            println!(
                "Condition '{}' met for {} '{}'",
                condition,
                target.as_str(),
                name
            );
            return Ok(());
        }

        if start.elapsed().as_secs() >= timeout_secs {
            return Err(anyhow::anyhow!(
                "Timed out after {}s waiting for condition '{}' on {} '{}'",
                timeout_secs,
                condition,
                target.as_str(),
                name
            ));
        }

        tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECONDS)).await;
    }
}

/// Whether a service satisfies `condition`.
///
/// The expected task count depends on the service mode: a replicated service has
/// `Spec.Mode.Replicated.Replicas`, but a *global* service has no replica count
/// and is expected to run one task per node. Treating a global service as
/// `desired = 0` made `running >= desired` trivially true, so `wait` reported
/// success instantly even with nothing running.
pub fn service_satisfied(
    condition: &str,
    service_exists: bool,
    mode: Option<&bollard::models::ServiceSpecMode>,
    node_count: usize,
    tasks: &[bollard::models::Task],
) -> bool {
    let condition = condition.to_lowercase();
    if condition == "deleted" || condition == "removed" {
        return !service_exists;
    }
    if !service_exists {
        return false;
    }
    if condition == "exists" {
        return true;
    }

    // Count tasks by actual state, not desired state.
    let running = crate::api::task::count_running(tasks) as i64;

    let expected: i64 = match mode {
        Some(m) if m.global.is_some() => node_count as i64,
        Some(m) => m.replicated.as_ref().and_then(|r| r.replicas).unwrap_or(0),
        None => 0,
    };

    running >= expected
}

async fn check_service_ready(
    client: &DockerClient,
    name: &str,
    condition: &str,
) -> anyhow::Result<bool> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let service = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_deref()) == Some(name));

    // Only needed to evaluate a global service's expectation.
    let node_count = crate::api::node::list_nodes(client.inner())
        .await
        .map(|n| n.len())
        .unwrap_or(0);

    match service {
        Some(s) => {
            let spec = s.spec.as_ref();
            let mode = spec.and_then(|sp| sp.mode.as_ref());
            let tasks = match crate::api::task::tasks_for_service(client.inner(), name).await {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("Could not list tasks for service '{}': {}", name, e);
                    Vec::new()
                }
            };
            Ok(service_satisfied(condition, true, mode, node_count, &tasks))
        }
        None => {
            Ok(condition.eq_ignore_ascii_case("deleted")
                || condition.eq_ignore_ascii_case("removed"))
        }
    }
}

async fn check_task_ready(
    client: &DockerClient,
    name: &str,
    condition: &str,
) -> anyhow::Result<bool> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;
    let task = tasks
        .into_iter()
        .find(|t| t.id.as_deref() == Some(name) || t.name.as_deref() == Some(name));

    let condition = condition.to_lowercase();
    match task {
        Some(t) => {
            let state = t.status.as_ref().and_then(|s| s.state);
            match condition.as_str() {
                "running" => Ok(matches!(state, Some(bollard::models::TaskState::RUNNING))),
                "complete" | "completed" => {
                    Ok(matches!(state, Some(bollard::models::TaskState::COMPLETE)))
                }
                "exists" => Ok(true),
                "deleted" | "removed" => Ok(false),
                _ => Ok(false),
            }
        }
        None => Ok(matches!(condition.as_str(), "deleted" | "removed")),
    }
}

async fn check_node_ready(
    client: &DockerClient,
    name: &str,
    condition: &str,
) -> anyhow::Result<bool> {
    let node = crate::api::node::find_node_opt(client.inner(), name).await?;

    let condition = condition.to_lowercase();
    match node {
        Some(n) => {
            let status = n.status.as_ref().and_then(|s| s.state);
            let availability = n.spec.as_ref().and_then(|s| s.availability);
            match condition.as_str() {
                "ready" => Ok(matches!(status, Some(bollard::models::NodeState::READY))),
                "down" => Ok(matches!(status, Some(bollard::models::NodeState::DOWN))),
                "drained" => Ok(matches!(
                    availability,
                    Some(bollard::models::NodeSpecAvailabilityEnum::DRAIN)
                )),
                "cordoned" => Ok(matches!(
                    availability,
                    Some(bollard::models::NodeSpecAvailabilityEnum::DRAIN)
                )),
                "exists" => Ok(true),
                "deleted" | "removed" => Ok(false),
                _ => Ok(false),
            }
        }
        None => Ok(matches!(condition.as_str(), "deleted" | "removed")),
    }
}

/// True when `resource` is a kind `wait` can handle.
///
/// The CLI enum and the string resolver must agree, or `wait nodes` would be
/// accepted by clap and then rejected at runtime.
pub fn resource_type_supported(resource: WaitResourceType) -> bool {
    matches!(
        resource,
        WaitResourceType::Service | WaitResourceType::Task | WaitResourceType::Node
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::root::WaitResourceType;
    use bollard::models::{
        ServiceSpecMode, ServiceSpecModeReplicated, Task, TaskSpec, TaskSpecContainerSpec,
        TaskState, TaskStatus,
    };

    fn task(state: TaskState) -> Task {
        Task {
            id: Some("t1".into()),
            status: Some(TaskStatus {
                state: Some(state),
                ..Default::default()
            }),
            spec: Some(TaskSpec {
                container_spec: Some(TaskSpecContainerSpec::default()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn replicated(n: i64) -> ServiceSpecMode {
        ServiceSpecMode {
            replicated: Some(ServiceSpecModeReplicated { replicas: Some(n) }),
            ..Default::default()
        }
    }

    #[test]
    fn resolves_aliases() {
        assert_eq!(resolve_resource_type("svc").unwrap(), WaitTarget::Service);
        assert_eq!(
            resolve_resource_type("Service").unwrap(),
            WaitTarget::Service
        );
        assert_eq!(resolve_resource_type("po").unwrap(), WaitTarget::Task);
        assert_eq!(resolve_resource_type("NO").unwrap(), WaitTarget::Node);
        assert!(resolve_resource_type("volumes").is_err());
    }

    #[test]
    fn rejects_unknown_conditions() {
        assert!(validate_condition(WaitTarget::Service, "available").is_ok());
        assert!(validate_condition(WaitTarget::Service, "AVAILABLE").is_ok());
        // The old code accepted anything and reported success immediately.
        assert!(validate_condition(WaitTarget::Service, "typo-condition").is_err());
        assert!(validate_condition(WaitTarget::Task, "available").is_err());
        assert!(validate_condition(WaitTarget::Node, "running").is_err());
    }

    #[test]
    fn replicated_service_waits_for_running_tasks() {
        let mode = replicated(3);
        assert!(!service_satisfied(
            "available",
            true,
            Some(&mode),
            5,
            &[task(TaskState::RUNNING), task(TaskState::RUNNING)]
        ));
        assert!(service_satisfied(
            "available",
            true,
            Some(&mode),
            5,
            &[
                task(TaskState::RUNNING),
                task(TaskState::RUNNING),
                task(TaskState::RUNNING),
            ]
        ));
    }

    fn global_mode() -> ServiceSpecMode {
        ServiceSpecMode {
            global: Some(std::collections::HashMap::from([((), ())])),
            ..Default::default()
        }
    }

    #[test]
    fn global_service_waits_for_one_task_per_node() {
        let mode = global_mode();
        // 3 nodes, nothing running: must NOT be satisfied.
        assert!(!service_satisfied("available", true, Some(&mode), 3, &[]));
        assert!(!service_satisfied(
            "available",
            true,
            Some(&mode),
            3,
            &[task(TaskState::RUNNING)]
        ));
        assert!(service_satisfied(
            "available",
            true,
            Some(&mode),
            3,
            &[
                task(TaskState::RUNNING),
                task(TaskState::RUNNING),
                task(TaskState::RUNNING),
            ]
        ));
    }

    #[test]
    fn failed_tasks_do_not_count_towards_availability() {
        let mode = replicated(2);
        // desired_state says RUNNING for both, but both actually failed.
        let mut t1 = task(TaskState::FAILED);
        t1.desired_state = Some(TaskState::RUNNING);
        let mut t2 = task(TaskState::COMPLETE);
        t2.desired_state = Some(TaskState::RUNNING);
        assert!(!service_satisfied(
            "available",
            true,
            Some(&mode),
            1,
            &[t1, t2]
        ));
    }

    #[test]
    fn zero_replica_service_is_satisfied_immediately() {
        let mode = replicated(0);
        assert!(service_satisfied("available", true, Some(&mode), 3, &[]));
    }

    #[test]
    fn deleted_condition_tracks_absence() {
        let mode = replicated(1);
        assert!(service_satisfied("deleted", false, None, 0, &[]));
        assert!(!service_satisfied("deleted", true, Some(&mode), 0, &[]));
        assert!(service_satisfied("removed", false, None, 0, &[]));
    }

    #[test]
    fn missing_service_is_not_available_but_is_deleted() {
        assert!(!service_satisfied("available", false, None, 0, &[]));
        assert!(service_satisfied("exists", true, None, 0, &[]));
        assert!(!service_satisfied("exists", false, None, 0, &[]));
    }

    #[test]
    fn wait_resource_types_match_the_supported_set() {
        // The clap enum Displays a singular name ("service"); the resolver
        // normalises it to the canonical plural used in output.
        let cases = [
            (WaitResourceType::Service, "services"),
            (WaitResourceType::Task, "tasks"),
            (WaitResourceType::Node, "nodes"),
        ];
        for (rt, canonical) in cases {
            let displayed = rt.to_string();
            assert!(resource_type_supported(rt));
            assert_eq!(
                resolve_resource_type(&displayed).unwrap().as_str(),
                canonical
            );
        }
    }
}
