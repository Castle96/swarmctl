use crate::api::client::DockerClient;
use bollard::Docker;
use bollard::models::Task;

/// Whether a task's **actual** state is running.
///
/// `Task.desired_state` is what the scheduler *wants*, not what is happening.
/// A crash-looping task reports `desired_state = running` while its
/// `status.state` cycles through `COMPLETE`/`FAILED`. Counting desired state
/// therefore reports failed containers as healthy — previously done in 19
/// places across `top`, `wait`, `rollout`, `exec`, `attach`, `cp` and
/// `port_forward`.
pub fn is_running(state: &bollard::models::TaskState) -> bool {
    matches!(state, bollard::models::TaskState::RUNNING)
}

/// A task's actual state, or `None` when Docker has not reported one.
pub fn actual_state(task: &Task) -> Option<bollard::models::TaskState> {
    task.status.as_ref().and_then(|s| s.state)
}

/// Whether a task is actually running right now.
pub fn task_is_running(task: &Task) -> bool {
    actual_state(task).map(|s| is_running(&s)).unwrap_or(false)
}

/// Count tasks of a service that are actually running.
pub fn count_running(tasks: &[Task]) -> usize {
    tasks.iter().filter(|t| task_is_running(t)).count()
}

/// Container id of a running task belonging to `service`.
///
/// Returns the most recently started running task, falling back to any task so
/// callers that merely need *a* container (exec, cp, attach) still work for
/// services whose tasks are momentarily restarting.
pub fn running_task_container(tasks: &[Task], service: &str) -> Option<(String, String)> {
    let for_service = |t: &&Task| {
        t.service_id.as_deref() == Some(service)
            || t.spec
                .as_ref()
                .and_then(|s| s.container_spec.as_ref())
                .and_then(|c| c.labels.as_ref())
                .and_then(|l| l.get("com.docker.swarm.service.name"))
                .is_some_and(|v| v == service)
    };

    let mut candidates: Vec<&Task> = tasks.iter().filter(for_service).collect();
    if candidates.is_empty() {
        return None;
    }
    // Newest first: task names embed a monotonically increasing slot index, but
    // the task id sorts most reliably for "most recent".
    candidates.sort_by(|a, b| b.id.cmp(&a.id));

    let pick = candidates
        .iter()
        .find(|t| task_is_running(t))
        .or(candidates.first())?;

    let task_id = pick.id.clone()?;
    let container = pick
        .status
        .as_ref()
        .and_then(|s| s.container_status.as_ref())
        .and_then(|c| c.container_id.as_ref())
        .cloned()?;

    Some((task_id, container))
}

/// Resolve a service name to its ID.
pub async fn service_id_for(docker: &Docker, service_name: &str) -> anyhow::Result<String> {
    let services = crate::api::service::list_services(docker).await?;
    services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_deref()) == Some(service_name))
        .and_then(|s| s.id)
        .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", service_name))
}

/// Find a running container for a service, connecting first if needed.
pub async fn running_container_for_service(
    client: &DockerClient,
    service_name: &str,
) -> anyhow::Result<(String, String)> {
    let service_id = service_id_for(client.inner(), service_name).await?;
    let mut filters = std::collections::HashMap::new();
    filters.insert("service".to_string(), vec![service_id]);
    let opts = bollard::query_parameters::ListTasksOptions {
        filters: Some(filters),
    };
    let tasks = client.inner().list_tasks(Some(opts)).await?;
    running_task_container(&tasks, service_name).ok_or_else(|| {
        anyhow::anyhow!(
            "No running task found for service '{}'. Is it deployed and running?",
            service_name
        )
    })
}

pub async fn list_tasks(docker: &Docker) -> anyhow::Result<Vec<Task>> {
    Ok(docker
        .list_tasks(None::<bollard::query_parameters::ListTasksOptions>)
        .await?)
}

/// All tasks belonging to one service.
pub async fn tasks_for_service(docker: &Docker, service_name: &str) -> anyhow::Result<Vec<Task>> {
    let service_id = service_id_for(docker, service_name).await?;
    let mut filters = std::collections::HashMap::new();
    filters.insert("service".to_string(), vec![service_id]);
    let opts = bollard::query_parameters::ListTasksOptions {
        filters: Some(filters),
    };
    Ok(docker.list_tasks(Some(opts)).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{ContainerStatus, TaskState, TaskStatus};

    fn task(id: &str, desired: TaskState, actual: Option<TaskState>) -> Task {
        Task {
            id: Some(id.to_string()),
            service_id: Some("svc1".to_string()),
            desired_state: Some(desired),
            status: Some(TaskStatus {
                state: actual,
                container_status: Some(ContainerStatus {
                    container_id: Some(format!("ctr-{id}")),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn desired_state_running_but_actually_failed_is_not_running() {
        let t = task("1", TaskState::RUNNING, Some(TaskState::FAILED));
        assert!(!task_is_running(&t), "a crash-looping task is not running");
        assert_eq!(count_running(&[t]), 0);
    }

    #[test]
    fn desired_state_shutdown_but_actually_running_is_running() {
        let t = task("1", TaskState::SHUTDOWN, Some(TaskState::RUNNING));
        assert!(task_is_running(&t));
    }

    #[test]
    fn missing_status_is_not_running() {
        let mut t = task("1", TaskState::RUNNING, Some(TaskState::RUNNING));
        t.status = None;
        assert!(!task_is_running(&t));
    }

    #[test]
    fn counts_only_actually_running_tasks() {
        let tasks = vec![
            task("1", TaskState::RUNNING, Some(TaskState::RUNNING)),
            task("2", TaskState::RUNNING, Some(TaskState::FAILED)),
            task("3", TaskState::RUNNING, Some(TaskState::COMPLETE)),
            task("4", TaskState::RUNNING, Some(TaskState::RUNNING)),
        ];
        assert_eq!(count_running(&tasks), 2);
    }

    #[test]
    fn picks_running_task_for_the_right_service() {
        let tasks = vec![
            task("a", TaskState::RUNNING, Some(TaskState::FAILED)),
            task("b", TaskState::RUNNING, Some(TaskState::RUNNING)),
        ];
        let (id, container) = running_task_container(&tasks, "svc1").unwrap();
        assert_eq!(id, "b");
        assert_eq!(container, "ctr-b");
    }

    #[test]
    fn falls_back_to_non_running_task_when_none_are_up() {
        // exec/cp still need a container id during a rolling restart.
        let tasks = vec![task("a", TaskState::RUNNING, Some(TaskState::COMPLETE))];
        assert!(running_task_container(&tasks, "svc1").is_some());
    }

    #[test]
    fn returns_none_for_unknown_service() {
        let tasks = vec![task("a", TaskState::RUNNING, Some(TaskState::RUNNING))];
        assert!(running_task_container(&tasks, "other").is_none());
        assert!(running_task_container(&[], "svc1").is_none());
    }

    #[test]
    fn matches_service_via_task_template_label() {
        let mut t = task("z", TaskState::RUNNING, Some(TaskState::RUNNING));
        t.service_id = Some("different-id".to_string());
        t.spec = Some(bollard::models::TaskSpec {
            container_spec: Some(bollard::models::TaskSpecContainerSpec {
                labels: Some(
                    [(
                        "com.docker.swarm.service.name".to_string(),
                        "svc1".to_string(),
                    )]
                    .into_iter()
                    .collect(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        });
        assert!(running_task_container(&[t], "svc1").is_some());
    }
}
