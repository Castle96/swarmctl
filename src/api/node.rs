use bollard::Docker;
use bollard::models::Node;

pub async fn list_nodes(docker: &Docker) -> anyhow::Result<Vec<Node>> {
    Ok(docker
        .list_nodes(None::<bollard::query_parameters::ListNodesOptions>)
        .await?)
}