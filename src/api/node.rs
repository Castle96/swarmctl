use bollard::Docker;
use bollard::models::Node;
use bollard::query_parameters::ListNodesOptions;

pub async fn list_nodes(docker: &Docker) -> anyhow::Result<Vec<Node>> {
    Ok(docker
        .list_nodes(None::<ListNodesOptions<String>>)
        .await?)
}