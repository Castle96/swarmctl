use crate::api::client::DockerClient;
use serde_json::Value;

pub async fn run(
    client: &DockerClient,
    resource: &str,
    name: String,
    patch_content: String,
    dry_run: bool,
    patch_type: Option<String>,
) -> anyhow::Result<()> {
    if let Some(t) = patch_type.as_deref() {
        // Previously the flag was accepted and discarded, so `--patch-type json`
        // silently performed a merge patch.
        if t != "merge" && t != "strategic" {
            anyhow::bail!(
                "Unsupported --patch-type '{}'. Only 'merge' (RFC 7386) is implemented.",
                t
            );
        }
    }

    let patch: Value = serde_json::from_str(&patch_content)?;

    match resource {
        "service" | "services" => {
            let service = crate::api::service::find_service(client.inner(), &name).await?;

            let version = service.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i32;
            let current = serde_json::to_value(&service)?;
            let merged = json_merge(
                json_merge(Value::Object(Default::default()), current),
                patch,
            );
            let patched: bollard::models::Service = serde_json::from_value(merged.clone())?;
            let spec = patched.spec.unwrap_or_default();

            if dry_run {
                print_dry_run("Service", &name, &merged, &serde_json::to_value(&spec)?);
                return Ok(());
            }

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
            let node = crate::api::node::find_node(client.inner(), &name).await?;
            let version = node.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i64;
            let current = serde_json::to_value(&node)?;
            let merged = json_merge(
                json_merge(Value::Object(Default::default()), current),
                patch,
            );
            let patched: bollard::models::Node = serde_json::from_value(merged.clone())?;
            let spec = patched.spec.unwrap_or_default();

            if dry_run {
                print_dry_run("Node", &name, &merged, &serde_json::to_value(&spec)?);
                return Ok(());
            }

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

/// Show what a patch would change without applying it.
///
/// `--dry-run` used to be parsed and discarded, so the patch was applied to the
/// cluster anyway.
fn print_dry_run(kind: &str, name: &str, merged: &Value, spec: &Value) {
    println!("[dry run] Would patch {} '{}'", kind, name);
    println!("  Resulting spec:");
    let rendered =
        serde_json::to_string_pretty(spec).unwrap_or_else(|e| format!("<unserializable: {}>", e));
    for line in rendered.lines() {
        println!("    {}", line);
    }
    let _ = merged;
}

/// RFC 7386 JSON merge patch: objects merge recursively, a `null` value deletes
/// the key, and any non-object replaces the target outright.
pub fn json_merge(a: Value, b: Value) -> Value {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merges_nested_objects() {
        let got = json_merge(json!({"a": {"b": 1, "c": 2}}), json!({"a": {"c": 3}}));
        assert_eq!(got, json!({"a": {"b": 1, "c": 3}}));
    }

    #[test]
    fn null_deletes_a_key() {
        let got = json_merge(json!({"a": 1, "b": 2}), json!({"b": null}));
        assert_eq!(got, json!({"a": 1}));
    }

    #[test]
    fn non_object_replaces_outright() {
        assert_eq!(json_merge(json!({"a": 1}), json!([1, 2])), json!([1, 2]));
        assert_eq!(json_merge(json!({"a": {"b": 1}}), json!("s")), json!("s"));
    }

    #[test]
    fn adds_missing_keys() {
        assert_eq!(json_merge(json!({}), json!({"a": 1})), json!({"a": 1}));
    }

    #[test]
    fn merge_into_empty_is_identity_on_the_patch() {
        let patch = json!({"Spec": {"Labels": {"env": "prod"}}});
        assert_eq!(
            json_merge(Value::Object(Default::default()), patch.clone()),
            patch
        );
    }

    #[test]
    fn arrays_are_replaced_not_merged() {
        let got = json_merge(json!({"a": [1, 2, 3]}), json!({"a": [9]}));
        assert_eq!(got, json!({"a": [9]}));
    }
}
