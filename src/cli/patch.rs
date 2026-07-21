use crate::api::client::DockerClient;

pub async fn run(
    client: &DockerClient,
    resource: &str,
    name: String,
    patch_content: String,
) -> anyhow::Result<()> {
    let patch: serde_json::Value = serde_json::from_str(&patch_content)?;

    match resource {
        "service" | "services" => {
            let services = crate::api::service::list_services(client.inner()).await?;
            let service = services
                .into_iter()
                .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name))
                .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", name))?;

            let version = service.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i32;
            let json = serde_json::to_value(&service)?;
            let merged = serde_json::json!({});
            let merged = json_merge(merged, json);
            let merged = json_merge(merged, patch);
            let patched: bollard::models::Service = serde_json::from_value(merged)?;
            let spec = patched.spec.unwrap_or_default();

            let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
                .version(version)
                .build();
            client
                .inner()
                .update_service(&name, spec, opts, None)
                .await?;
            println!("Service '{}' patched", name);
        }
        "node" | "nodes" => {
            let nodes = crate::api::node::list_nodes(client.inner()).await?;
            let node = nodes
                .into_iter()
                .find(|n| n.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name))
                .ok_or_else(|| anyhow::anyhow!("Node '{}' not found", name))?;

            let version = node.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i64;
            let json = serde_json::to_value(&node)?;
            let merged = serde_json::json!({});
            let merged = json_merge(merged, json);
            let merged = json_merge(merged, patch);
            let patched: bollard::models::Node = serde_json::from_value(merged)?;
            let spec = patched.spec.unwrap_or_default();

            let opts = bollard::query_parameters::UpdateNodeOptionsBuilder::default()
                .version(version)
                .build();
            client.inner().update_node(&name, spec, opts).await?;
            println!("Node '{}' patched", name);
        }
        _ => anyhow::bail!("Unsupported resource type for patch: {}", resource),
    }

    Ok(())
}

fn json_merge(a: serde_json::Value, b: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match (a, b) {
        (Value::Object(mut a), Value::Object(b)) => {
            for (k, v) in b {
                if v.is_null() {
                    a.remove(&k);
                } else {
                    let existing = a.remove(&k).unwrap_or(Value::Null);
                    a.insert(k, json_merge(existing, v));
                }
            }
            Value::Object(a)
        }
        (_, b) => b,
    }
}
