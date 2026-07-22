use bollard::Docker;
use bollard::models::{Node, Task, NodeState, TaskState};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use crate::models::failover::{FailoverEvent, FailoverState};

pub struct FailoverMonitor {
    state: Arc<Mutex<FailoverState>>,
    event_tx: mpsc::UnboundedSender<FailoverEvent>,
    known_nodes: HashMap<String, NodeState>,
    known_tasks: HashMap<String, TaskState>,
}

impl FailoverMonitor {
    pub fn new(event_tx: mpsc::UnboundedSender<FailoverEvent>) -> Self {
        Self {
            state: Arc::new(Mutex::new(FailoverState::default())),
            event_tx,
            known_nodes: HashMap::new(),
            known_tasks: HashMap::new(),
        }
    }

    pub fn get_state(&self) -> FailoverState {
        self.state.lock().unwrap().clone()
    }

    pub fn set_enabled(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        state.enabled = enabled;
    }

    pub fn check_and_update(
        &mut self,
        nodes: &[Node],
        tasks: &[Task],
    ) -> Vec<FailoverEvent> {
        let mut events = Vec::new();

        let mut current_nodes: HashMap<String, NodeState> = HashMap::new();
        for node in nodes {
            if let (Some(id), Some(status)) = (&node.id, &node.status) {
                if let Some(state) = &status.state {
                    current_nodes.insert(id.clone(), state.clone());
                }
            }
        }

        for (node_id, new_state) in &current_nodes {
            if let Some(old_state) = self.known_nodes.get(node_id) {
                if old_state != new_state {
                    match new_state {
                        NodeState::DOWN => {
                            let node_name = nodes.iter()
                                .find(|n| n.id.as_deref() == Some(node_id))
                                .and_then(|n| n.spec.as_ref())
                                .and_then(|s| s.name.as_deref())
                                .unwrap_or(node_id);

                            let event = FailoverEvent::NodeDown {
                                node_id: node_id.clone(),
                                node_name: node_name.to_string(),
                                timestamp: chrono_now(),
                            };
                            events.push(event.clone());
                            self.send_event(event);

                            let mut state = self.state.lock().unwrap();
                            state.failed_nodes.push(node_id.clone());
                        }
                        NodeState::READY => {
                            let mut state = self.state.lock().unwrap();
                            state.failed_nodes.retain(|n| n != node_id);
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut current_tasks: HashMap<String, TaskState> = HashMap::new();
        for task in tasks {
            if let (Some(id), Some(status)) = (&task.id, &task.status) {
                if let Some(state) = &status.state {
                    current_tasks.insert(id.clone(), state.clone());
                }
            }
        }

        for (task_id, new_state) in &current_tasks {
            if let Some(old_state) = self.known_tasks.get(task_id) {
                if old_state != &TaskState::RUNNING && new_state == &TaskState::RUNNING {
                    let task = tasks.iter().find(|t| t.id.as_deref() == Some(task_id));
                    if let Some(task) = task {
                        let service_name = task
                            .service_id
                            .as_deref()
                            .unwrap_or("unknown");
                        let target_node = task
                            .node_id
                            .as_deref()
                            .unwrap_or("unknown");

                        let event = FailoverEvent::ContainerRedeployed {
                            service_name: service_name.to_string(),
                            task_id: task_id.clone(),
                            target_node: target_node.to_string(),
                            timestamp: chrono_now(),
                        };
                        events.push(event.clone());
                        self.send_event(event);
                    }
                }
            }
        }

        self.known_nodes = current_nodes;
        self.known_tasks = current_tasks;

        events
    }

    fn send_event(&self, event: FailoverEvent) {
        let state = self.state.lock().unwrap();
        if state.enabled {
            let _ = self.event_tx.send(event);
        }
    }
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let secs = duration.as_secs();
    let mins = secs / 60;
    let hours = mins / 60;
    format!("{:02}:{:02}:{:02}", hours % 24, mins % 60, secs % 60)
}

#[allow(dead_code)]
pub async fn get_node_tasks(docker: &Docker, node_id: &str) -> anyhow::Result<Vec<Task>> {
    let mut filters = std::collections::HashMap::new();
    filters.insert("node".to_string(), vec![node_id.to_string()]);

    let options = bollard::query_parameters::ListTasksOptions {
        filters: Some(filters),
        ..Default::default()
    };

    Ok(docker.list_tasks(Some(options)).await?)
}

#[allow(dead_code)]
pub async fn get_service_tasks(docker: &Docker, service_id: &str) -> anyhow::Result<Vec<Task>> {
    let mut filters = std::collections::HashMap::new();
    filters.insert("service".to_string(), vec![service_id.to_string()]);

    let options = bollard::query_parameters::ListTasksOptions {
        filters: Some(filters),
        ..Default::default()
    };

    Ok(docker.list_tasks(Some(options)).await?)
}
