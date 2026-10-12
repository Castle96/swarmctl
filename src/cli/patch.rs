use crate::api::client::DockerClient;
use crate::cli::root::ResourceType;
use serde_json::Value;

/// Kinds `patch` can apply to.
///
/// One predicate, used by both the guard and the error message, so the help
/// text, the guard, and the dispatch cannot disagree. `patch` previously
/// documented four kinds and implemented two, so the CLI accepted
/// `patch secrets foo` and then refused it.
pub fn is_patchable(resource: ResourceType) -> bool {
    matches!(
        resource,
        ResourceType::Services
            | ResourceType::Nodes
            | ResourceType::Secrets
            | ResourceType::Configs
    )
}

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
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

    if !is_patchable(resource) {
        anyhow::bail!(
            "patch is not supported for {}. Patchable kinds: services, nodes, \
             secrets, configs.",
            resource
        );
    }

    let patch: Value = serde_json::from_str(&patch_content)?;

    match resource {
        ResourceType::Services => {
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
                print_dry_run("Service", &name, &serde_json::to_value(&spec)?);
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
        ResourceType::Nodes => {
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
                print_dry_run("Node", &name, &serde_json::to_value(&spec)?);
                return Ok(());
            }

            let opts = bollard::query_parameters::UpdateNodeOptionsBuilder::default()
                .version(version)
                .build();
            client.inner().update_node(&name, spec, opts).await?;
            println!("Node '{}' patched", name);
        }
        // The Engine API addresses secrets and configs by ID, not by name.
        ResourceType::Secrets => {
            let secret = crate::api::secret::find_secret(client.inner(), &name).await?;
            let id = require_id(&secret.id, "Secret", &name)?;
            let version = secret.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i64;
            let current = serde_json::to_value(&secret)?;
            let merged = json_merge(
                json_merge(Value::Object(Default::default()), current),
                patch,
            );
            let patched: bollard::models::Secret = serde_json::from_value(merged.clone())?;
            let spec = patched.spec.unwrap_or_default();

            if dry_run {
                print_dry_run("Secret", &name, &serde_json::to_value(&spec)?);
                return Ok(());
            }

            let opts = bollard::query_parameters::UpdateSecretOptionsBuilder::default()
                .version(version)
                .build();
            client.inner().update_secret(&id, spec, opts).await?;
            println!("Secret '{}' patched", name);
        }
        ResourceType::Configs => {
            let config = crate::api::config::find_config(client.inner(), &name).await?;
            let id = require_id(&config.id, "Config", &name)?;
            let version = config.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i64;
            let current = serde_json::to_value(&config)?;
            let merged = json_merge(
                json_merge(Value::Object(Default::default()), current),
                patch,
            );
            let patched: bollard::models::Config = serde_json::from_value(merged.clone())?;
            let spec = patched.spec.unwrap_or_default();

            if dry_run {
                print_dry_run("Config", &name, &serde_json::to_value(&spec)?);
                return Ok(());
            }

            let opts = bollard::query_parameters::UpdateConfigOptionsBuilder::default()
                .version(version)
                .build();
            client.inner().update_config(&id, spec, opts).await?;
            println!("Config '{}' patched", name);
        }
        // `is_patchable` above already rejected everything else.
        other => unreachable!("patch dispatch reached for {other:?}"),
    }

    Ok(())
}

/// Require the API id of an object looked up by name.
fn require_id(id: &Option<String>, kind: &str, name: &str) -> anyhow::Result<String> {
    id.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "{} '{}' has no id, so it cannot be patched. The daemon returned a \
             response without one.",
            kind,
            name
        )
    })
}

/// Show what a patch would change without applying it.
///
/// `--dry-run` used to be parsed and discarded, so the patch was applied to the
/// cluster anyway.
fn print_dry_run(kind: &str, name: &str, spec: &Value) {
    println!("[dry run] Would patch {} '{}'", kind, name);
    println!("  Resulting spec:");
    let rendered =
        serde_json::to_string_pretty(spec).unwrap_or_else(|e| format!("<unserializable: {}>", e));
    for line in rendered.lines() {
        println!("    {}", line);
    }
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

    /// `patch` documented four kinds and implemented two, so the CLI accepted
    /// `patch secrets foo` and then refused it. The help text and the dispatch
    /// must agree, and the kinds with no update endpoint must not be listed.
    #[test]
    fn patchable_kinds_match_what_is_documented() {
        for kind in [
            ResourceType::Services,
            ResourceType::Nodes,
            ResourceType::Secrets,
            ResourceType::Configs,
        ] {
            assert!(is_patchable(kind), "{kind} should be patchable");
        }
        for kind in [
            ResourceType::Tasks,
            ResourceType::Networks,
            ResourceType::Stacks,
        ] {
            assert!(
                !is_patchable(kind),
                "{kind} has no update endpoint and must not be advertised"
            );
        }
    }

    #[test]
    fn require_id_rejects_a_response_without_one() {
        assert_eq!(
            require_id(&Some("abc".into()), "Secret", "web").unwrap(),
            "abc"
        );
        let err = require_id(&None, "Secret", "web")
            .expect_err("a missing id should be reported, not sent as a name");
        assert!(err.to_string().contains("has no id"));
    }
}
