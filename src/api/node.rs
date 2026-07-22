use bollard::Docker;
use bollard::models::Node;
use bollard::query_parameters::UpdateNodeOptions;

pub async fn list_nodes(docker: &Docker) -> anyhow::Result<Vec<Node>> {
    Ok(docker
        .list_nodes(None::<bollard::query_parameters::ListNodesOptions>)
        .await?)
}

pub async fn inspect_node(docker: &Docker, node_id: &str) -> anyhow::Result<Node> {
    Ok(docker.inspect_node(node_id).await?)
}

pub async fn promote_node(docker: &Docker, node_id: &str) -> anyhow::Result<()> {
    let node = docker.inspect_node(node_id).await?;
    let version = node
        .version
        .and_then(|v| v.index)
        .unwrap_or(0);
    let mut spec = node.spec.unwrap_or_default();
    spec.role = Some(bollard::models::NodeSpecRoleEnum::MANAGER);

    let options = UpdateNodeOptions {
        version: version as i64,
    };
    docker.update_node(node_id, spec, options).await?;
    Ok(())
}

pub async fn demote_node(docker: &Docker, node_id: &str) -> anyhow::Result<()> {
    let node = docker.inspect_node(node_id).await?;
    let version = node
        .version
        .and_then(|v| v.index)
        .unwrap_or(0);
    let mut spec = node.spec.unwrap_or_default();
    spec.role = Some(bollard::models::NodeSpecRoleEnum::WORKER);

    let options = UpdateNodeOptions {
        version: version as i64,
    };
    docker.update_node(node_id, spec, options).await?;
    Ok(())
}

pub async fn set_availability(
    docker: &Docker,
    node_id: &str,
    availability: bollard::models::NodeSpecAvailabilityEnum,
) -> anyhow::Result<()> {
    let node = docker.inspect_node(node_id).await?;
    let version = node
        .version
        .and_then(|v| v.index)
        .unwrap_or(0);
    let mut spec = node.spec.unwrap_or_default();
    spec.availability = Some(availability);

    let options = UpdateNodeOptions {
        version: version as i64,
    };
    docker.update_node(node_id, spec, options).await?;
    Ok(())
}

pub async fn get_node_id_by_hostname(
    docker: &Docker,
    hostname: &str,
) -> anyhow::Result<Option<String>> {
    let nodes = list_nodes(docker).await?;
    for node in &nodes {
        if let Some(ref spec) = node.spec {
            if spec.name.as_deref() == Some(hostname) {
                return Ok(node.id.clone());
            }
        }
    }
    Ok(None)
}

pub async fn get_swarm_nodes(docker: &Docker) -> anyhow::Result<Vec<Node>> {
    let mut filter = std::collections::HashMap::new();
    filter.insert(
        "role".to_string(),
        vec!["manager".to_string(), "worker".to_string()],
    );
    let options = bollard::query_parameters::ListNodesOptions {
        filters: Some(filter),
        ..Default::default()
    };
    Ok(docker.list_nodes(Some(options)).await?)
}

pub async fn get_managers(docker: &Docker) -> anyhow::Result<Vec<Node>> {
    let mut filter = std::collections::HashMap::new();
    filter.insert("role".to_string(), vec!["manager".to_string()]);
    let options = bollard::query_parameters::ListNodesOptions {
        filters: Some(filter),
        ..Default::default()
    };
    Ok(docker.list_nodes(Some(options)).await?)
}

pub async fn get_workers(docker: &Docker) -> anyhow::Result<Vec<Node>> {
    let mut filter = std::collections::HashMap::new();
    filter.insert("role".to_string(), vec!["worker".to_string()]);
    let options = bollard::query_parameters::ListNodesOptions {
        filters: Some(filter),
        ..Default::default()
    };
    Ok(docker.list_nodes(Some(options)).await?)
}
