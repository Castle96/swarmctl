use bollard::Docker;
use bollard::models::Node;
use bollard::query_parameters::UpdateNodeOptions;

pub async fn list_nodes(docker: &Docker) -> anyhow::Result<Vec<Node>> {
    Ok(docker
        .list_nodes(None::<bollard::query_parameters::ListNodesOptions>)
        .await?)
}

/// A node's display name: `Description.Hostname`, falling back to `Spec.Name`.
///
/// `Spec.Name` is often empty for nodes that joined before a hostname was
/// recorded, so prefer the description.
pub fn node_hostname(node: &Node) -> Option<String> {
    node.description
        .as_ref()
        .and_then(|d| d.hostname.clone())
        .or_else(|| node.spec.as_ref().and_then(|s| s.name.clone()))
}

/// Find a node by hostname or id.
pub async fn find_node(docker: &Docker, name: &str) -> anyhow::Result<Node> {
    list_nodes(docker)
        .await?
        .into_iter()
        .find(|n| node_hostname(n).as_deref() == Some(name) || n.id.as_deref() == Some(name))
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found", name))
}

/// Find a node by hostname or id, or `None`.
pub async fn find_node_opt(docker: &Docker, name: &str) -> anyhow::Result<Option<Node>> {
    Ok(list_nodes(docker)
        .await?
        .into_iter()
        .find(|n| node_hostname(n).as_deref() == Some(name) || n.id.as_deref() == Some(name)))
}

pub async fn promote_node(docker: &Docker, node_id: &str) -> anyhow::Result<()> {
    let node = docker.inspect_node(node_id).await?;
    let version = node.version.and_then(|v| v.index).unwrap_or(0);
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
    let version = node.version.and_then(|v| v.index).unwrap_or(0);
    let mut spec = node.spec.unwrap_or_default();
    spec.role = Some(bollard::models::NodeSpecRoleEnum::WORKER);

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
        if let Some(ref spec) = node.spec
            && spec.name.as_deref() == Some(hostname)
        {
            return Ok(node.id.clone());
        }
    }
    Ok(None)
}

pub async fn get_managers(docker: &Docker) -> anyhow::Result<Vec<Node>> {
    let mut filter = std::collections::HashMap::new();
    filter.insert("role".to_string(), vec!["manager".to_string()]);
    let options = bollard::query_parameters::ListNodesOptions {
        filters: Some(filter),
    };
    Ok(docker.list_nodes(Some(options)).await?)
}

pub async fn get_workers(docker: &Docker) -> anyhow::Result<Vec<Node>> {
    let mut filter = std::collections::HashMap::new();
    filter.insert("role".to_string(), vec!["worker".to_string()]);
    let options = bollard::query_parameters::ListNodesOptions {
        filters: Some(filter),
    };
    Ok(docker.list_nodes(Some(options)).await?)
}
