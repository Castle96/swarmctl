use crate::api::{client::DockerClient, node};
use crate::models::node::NodeRow;
use crate::utils::printer::print_table;
use bollard::models::{NodeSpecAvailabilityEnum, NodeState, Reachability};

pub async fn list(client: &DockerClient) -> anyhow::Result<()> {
    let nodes = node::list_nodes(client.inner()).await?;

    let rows: Vec<NodeRow> = nodes
        .into_iter()
        .map(|n| {
            let spec = n.spec.unwrap_or_default();
            let status = n.status.unwrap_or_default();

            let manager = n.manager_status.as_ref().map(|m| {
                match m.reachability.unwrap_or(Reachability::UNKNOWN) {
                    Reachability::REACHABLE => "Reachable",
                    Reachability::UNREACHABLE => "Unavailable",
                    _ => "-",
                }
            }).unwrap_or("-");

            NodeRow {
                id: n.id.unwrap_or_default(),
                hostname: spec.name.unwrap_or_default(),
                status: status.state.unwrap_or(NodeState::READY).to_string(),
                availability: spec.availability
                    .unwrap_or(NodeSpecAvailabilityEnum::ACTIVE)
                    .to_string(),
                manager: manager.to_string(),
            }
        })
        .collect();

    print_table(rows);
    Ok(())
}