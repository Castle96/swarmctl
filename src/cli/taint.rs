use crate::api::client::DockerClient;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum TaintEffect {
    NoSchedule,
    PreferNoSchedule,
    NoExecute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NodeTaint {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<TaintEffect>,
}

async fn docker_socket_request(
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> anyhow::Result<serde_json::Value> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    let socket_path = std::env::var("DOCKER_HOST")
        .ok()
        .and_then(|h| {
            h.strip_prefix("unix://")
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "/var/run/docker.sock".to_string());

    let mut stream = UnixStream::connect(&socket_path).await?;

    let body_str = body
        .map(|b| serde_json::to_string(b).unwrap_or_default())
        .unwrap_or_default();

    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body_str.len(),
        body_str
    );

    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;

    let response_str = String::from_utf8_lossy(&response);
    let body_start = response_str
        .find("\r\n\r\n")
        .map(|i| i + 4)
        .unwrap_or(0);
    let resp_body = &response_str[body_start..];

    if resp_body.is_empty() {
        Ok(serde_json::Value::Null)
    } else {
        Ok(serde_json::from_str(resp_body)?)
    }
}

pub async fn run(
    client: &DockerClient,
    name: String,
    taints: Vec<String>,
    remove: Vec<String>,
    overwrite: bool,
) -> anyhow::Result<()> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;
    let node = nodes
        .into_iter()
        .find(|n| n.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name))
        .ok_or_else(|| anyhow::anyhow!("Node '{}' not found", name))?;

    let node_id = node
        .id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Node '{}' has no ID", name))?;
    let version = node.version.and_then(|v| v.index).unwrap_or(0);

    let node_json =
        docker_socket_request("GET", &format!("/nodes/{}", node_id), None).await?;

    let spec = node_json
        .get("Spec")
        .cloned()
        .unwrap_or(serde_json::Value::Object(Default::default()));

    let mut current_taints: Vec<NodeTaint> = spec
        .get("Taints")
        .and_then(|t| serde_json::from_value(t.clone()).ok())
        .unwrap_or_default();

    if overwrite {
        current_taints.clear();
    }

    for taint_str in &taints {
        let parts: Vec<&str> = taint_str.splitn(3, ':').collect();
        if parts.len() != 3 {
            return Err(anyhow::anyhow!(
                "Invalid taint format: '{}'. Expected key=value:effect",
                taint_str
            ));
        }
        let key = parts[0].to_string();
        let value = parts[1].to_string();
        let effect = match parts[2] {
            "NoSchedule" => TaintEffect::NoSchedule,
            "PreferNoSchedule" => TaintEffect::PreferNoSchedule,
            "NoExecute" => TaintEffect::NoExecute,
            other => {
                return Err(anyhow::anyhow!(
                    "Invalid taint effect: '{}'. Valid: NoSchedule, PreferNoSchedule, NoExecute",
                    other
                ));
            }
        };

        if !overwrite {
            current_taints.retain(|t| t.key.as_deref() != Some(&key));
        }

        current_taints.push(NodeTaint {
            key: Some(key),
            value: Some(value),
            effect: Some(effect),
        });
    }

    for remove_str in &remove {
        let key = if remove_str.contains(':') {
            remove_str.splitn(2, ':').next().unwrap_or(remove_str)
        } else {
            remove_str
        };
        current_taints.retain(|t| t.key.as_deref() != Some(key));
    }

    let mut spec = spec
        .as_object()
        .cloned()
        .unwrap_or_default();
    spec.insert(
        "Taints".to_string(),
        serde_json::to_value(&current_taints)?,
    );

    let mut full_node = node_json
        .as_object()
        .cloned()
        .unwrap_or_default();
    full_node.insert("Spec".to_string(), serde_json::Value::Object(spec));

    let path = format!("/nodes/{}?version={}", node_id, version);
    docker_socket_request("POST", &path, Some(&serde_json::Value::Object(full_node)))
        .await?;

    if !taints.is_empty() {
        println!("Node '{}' taints updated", name);
    } else if !remove.is_empty() {
        println!("Node '{}' taints removed", name);
    }

    Ok(())
}
