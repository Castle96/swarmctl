use crate::models::failover::{FailoverEvent, FailoverState};
use bollard::models::{Node, NodeState, Task, TaskState};
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Watches for node and task state transitions.
///
/// `state` is a plain field, not `Arc<Mutex<_>>`. Every method that touches it
/// either takes `&mut self` or is `&self` on a monitor that is never shared, so
/// the interior mutability bought nothing — and the `Arc` was there only because
/// `set_enabled` and `send_event` took `&self`. In `cli::failover.rs` that same
/// shape on a second field had already caused a self-deadlock, documented in that
/// file; removing the lock removes the failure mode.
pub struct FailoverMonitor {
    state: FailoverState,
    event_tx: mpsc::UnboundedSender<FailoverEvent>,
    known_nodes: HashMap<String, NodeState>,
    known_tasks: HashMap<String, TaskState>,
}

impl FailoverMonitor {
    pub fn new(event_tx: mpsc::UnboundedSender<FailoverEvent>) -> Self {
        Self {
            state: FailoverState::default(),
            event_tx,
            known_nodes: HashMap::new(),
            known_tasks: HashMap::new(),
        }
    }

    pub fn get_state(&self) -> FailoverState {
        self.state.clone()
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.state.enabled = enabled;
    }

    /// Seed `failed_nodes` from the current node list without emitting events.
    ///
    /// `check_and_update` only fires on a *transition*, so a single sample could
    /// never report a node that is down right now. This reflects the present
    /// state so `status` is accurate without waiting for a change.
    pub fn observe_current(&mut self, nodes: &[Node]) {
        let state = &mut self.state;
        state.failed_nodes.clear();
        for node in nodes {
            let is_down = node
                .status
                .as_ref()
                .and_then(|s| s.state)
                .is_some_and(|s| s == NodeState::DOWN);
            if is_down && let Some(id) = &node.id {
                state.failed_nodes.push(id.clone());
            }
        }
        state.failed_nodes.sort();
    }

    pub fn check_and_update(&mut self, nodes: &[Node], tasks: &[Task]) -> Vec<FailoverEvent> {
        let mut events = Vec::new();

        let mut current_nodes: HashMap<String, NodeState> = HashMap::new();
        for node in nodes {
            if let (Some(id), Some(status)) = (&node.id, &node.status)
                && let Some(state) = &status.state
            {
                current_nodes.insert(id.clone(), *state);
            }
        }

        for (node_id, new_state) in &current_nodes {
            if let Some(old_state) = self.known_nodes.get(node_id)
                && old_state != new_state
            {
                match new_state {
                    NodeState::DOWN => {
                        let node_name = nodes
                            .iter()
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

                        let state = &mut self.state;
                        // A flapping node (DOWN -> DISCONNECTED -> DOWN) reached
                        // this twice; pushing unconditionally listed it twice.
                        if !state.failed_nodes.iter().any(|n| n == node_id) {
                            state.failed_nodes.push(node_id.clone());
                        }
                    }
                    NodeState::READY => {
                        let state = &mut self.state;
                        state.failed_nodes.retain(|n| n != node_id);
                    }
                    _ => {}
                }
            }
        }

        let mut current_tasks: HashMap<String, TaskState> = HashMap::new();
        for task in tasks {
            if let (Some(id), Some(status)) = (&task.id, &task.status)
                && let Some(state) = &status.state
            {
                current_tasks.insert(id.clone(), *state);
            }
        }

        for (task_id, new_state) in &current_tasks {
            if let Some(old_state) = self.known_tasks.get(task_id)
                && old_state != &TaskState::RUNNING
                && new_state == &TaskState::RUNNING
            {
                let task = tasks.iter().find(|t| t.id.as_deref() == Some(task_id));
                if let Some(task) = task {
                    let service_name = task.service_id.as_deref().unwrap_or("unknown");
                    let target_node = task.node_id.as_deref().unwrap_or("unknown");

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

        self.known_nodes = current_nodes;
        self.known_tasks = current_tasks;

        events
    }

    fn send_event(&self, event: FailoverEvent) {
        if self.state.enabled {
            let _ = self.event_tx.send(event);
        }
    }
}

fn chrono_now() -> String {
    crate::utils::time::hms(crate::utils::time::epoch_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{NodeSpec, NodeStatus};

    fn node(id: &str, name: &str, state: NodeState) -> Node {
        Node {
            id: Some(id.to_string()),
            spec: Some(NodeSpec {
                name: Some(name.to_string()),
                ..Default::default()
            }),
            status: Some(NodeStatus {
                state: Some(state),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn monitor() -> FailoverMonitor {
        let (tx, _rx) = mpsc::unbounded_channel();
        FailoverMonitor::new(tx)
    }

    #[test]
    fn first_observation_emits_no_transition_events() {
        let mut m = monitor();
        let nodes = vec![node("n1", "node1", NodeState::DOWN)];
        // A fresh monitor has no prior state, so nothing "changed".
        assert!(m.check_and_update(&nodes, &[]).is_empty());
    }

    #[test]
    fn transition_to_down_emits_one_event_and_records_the_node() {
        let mut m = monitor();
        m.check_and_update(&[node("n1", "node1", NodeState::READY)], &[]);

        let events = m.check_and_update(&[node("n1", "node1", NodeState::DOWN)], &[]);
        assert_eq!(events.len(), 1);
        match &events[0] {
            FailoverEvent::NodeDown {
                node_id, node_name, ..
            } => {
                assert_eq!(node_id, "n1");
                assert_eq!(node_name, "node1");
            }
            other => panic!("unexpected event: {other:?}"),
        }
        assert_eq!(m.get_state().failed_nodes, vec!["n1".to_string()]);
    }

    #[test]
    fn a_flapping_node_is_listed_once() {
        let mut m = monitor();
        let ready = || vec![node("n1", "node1", NodeState::READY)];
        let down = || vec![node("n1", "node1", NodeState::DOWN)];

        m.check_and_update(&ready(), &[]);
        m.check_and_update(&down(), &[]);
        m.check_and_update(&[node("n1", "node1", NodeState::UNKNOWN)], &[]);
        m.check_and_update(&down(), &[]);

        assert_eq!(
            m.get_state().failed_nodes.len(),
            1,
            "node must not be duplicated after flapping"
        );
    }

    #[test]
    fn recovery_removes_the_node_from_failed_nodes() {
        let mut m = monitor();
        m.check_and_update(&[node("n1", "node1", NodeState::READY)], &[]);
        m.check_and_update(&[node("n1", "node1", NodeState::DOWN)], &[]);
        assert_eq!(m.get_state().failed_nodes.len(), 1);

        m.check_and_update(&[node("n1", "node1", NodeState::READY)], &[]);
        assert!(m.get_state().failed_nodes.is_empty());
    }

    #[test]
    fn observe_current_reports_a_node_down_without_a_transition() {
        let mut m = monitor();
        m.observe_current(&[
            node("n1", "node1", NodeState::DOWN),
            node("n2", "node2", NodeState::READY),
        ]);
        assert_eq!(m.get_state().failed_nodes, vec!["n1".to_string()]);
    }

    #[test]
    fn observe_current_replaces_stale_entries() {
        let mut m = monitor();
        m.observe_current(&[node("n1", "node1", NodeState::DOWN)]);
        assert_eq!(m.get_state().failed_nodes.len(), 1);
        m.observe_current(&[node("n1", "node1", NodeState::READY)]);
        assert!(m.get_state().failed_nodes.is_empty());
    }

    #[test]
    fn monitoring_is_opt_in() {
        // Default is disabled: there is no automatic remediation behind the flag.
        let m = monitor();
        assert!(!m.get_state().enabled);
    }

    #[test]
    fn events_are_only_sent_when_enabled() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut m = FailoverMonitor::new(tx);
        m.set_enabled(false);

        m.check_and_update(&[node("n1", "node1", NodeState::READY)], &[]);
        m.check_and_update(&[node("n1", "node1", NodeState::DOWN)], &[]);
        assert!(rx.try_recv().is_err(), "disabled monitor must not emit");

        m.set_enabled(true);
        m.check_and_update(&[node("n1", "node1", NodeState::READY)], &[]);
        m.check_and_update(&[node("n1", "node1", NodeState::DOWN)], &[]);
        assert!(rx.try_recv().is_ok(), "enabled monitor must emit");
    }
}
