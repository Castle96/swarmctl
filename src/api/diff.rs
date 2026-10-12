//! Cross-cluster comparison.
//!
//! Compares one resource kind between two endpoints and reports what differs.
//! Comparison works on the rendered row fields, so it stays consistent with
//! `get` output and needs no separate notion of each resource's schema.

use crate::api::endpoints::Endpoint;
use crate::cli::root::ResourceType;
use crate::models::endpoint::{ClusterDiff, FieldDiff, NONE_CELL, ResourceDiff};
use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeMap;

/// Resources that can be compared across clusters.
pub fn comparable_resources() -> &'static [ResourceType] {
    &[
        ResourceType::Nodes,
        ResourceType::Services,
        ResourceType::Networks,
        ResourceType::Secrets,
        ResourceType::Configs,
        ResourceType::Stacks,
    ]
}

/// Whether a resource kind can meaningfully be diffed across clusters.
///
/// Tasks are excluded: task ids are per-run, so every cluster would appear
/// wholly different.
pub fn is_comparable(resource: ResourceType) -> bool {
    comparable_resources().contains(&resource)
}

pub async fn diff(
    left: &Endpoint,
    right: &Endpoint,
    resource: ResourceType,
) -> Result<ClusterDiff> {
    let mut result = ClusterDiff {
        left: left.name.clone(),
        right: right.name.clone(),
        ..Default::default()
    };

    if !is_comparable(resource) {
        anyhow::bail!(
            "{} cannot be compared across clusters: task ids are per-run, so every cluster \
would look wholly different. Comparable kinds: nodes, services, networks, secrets, configs, \
stacks.",
            format!("{:?}", resource).to_lowercase()
        );
    }

    // Both sides in parallel, each bounded. They used to be fetched
    // sequentially with no timeout at all, so an unresponsive `ssh://` endpoint
    // hung `diff-clusters` forever while the same endpoint is capped at 30s
    // under `--endpoints` fan-out.
    let (left_rows, right_rows) = tokio::join!(
        with_timeout(fetch(left, resource), FETCH_TIMEOUT),
        with_timeout(fetch(right, resource), FETCH_TIMEOUT),
    );

    let left_rows = match left_rows {
        Ok(rows) => rows,
        Err(e) => {
            result.left_unreachable = Some(e.to_string());
            return Ok(result);
        }
    };
    let right_rows = match right_rows {
        Ok(rows) => rows,
        Err(e) => {
            result.right_unreachable = Some(e.to_string());
            return Ok(result);
        }
    };

    result.diffs = compare(
        &left_rows,
        &right_rows,
        &format!("{:?}", resource).to_lowercase(),
    );
    result.identical = result.diffs.is_empty();
    Ok(result)
}

/// How long one endpoint gets to answer a diff fetch.
///
/// Matches the fan-out endpoint timeout, so the same cluster is given the same
/// budget whichever multi-cluster command is used.
pub const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Run a future with a deadline, reporting the timeout as an ordinary error.
async fn with_timeout<F, T>(future: F, limit: std::time::Duration) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    match tokio::time::timeout(limit, future).await {
        Ok(result) => result,
        Err(_) => anyhow::bail!("timed out after {}s", limit.as_secs()),
    }
}

/// Fetch a resource kind from one endpoint as name -> row JSON.
async fn fetch(endpoint: &Endpoint, resource: ResourceType) -> Result<BTreeMap<String, Value>> {
    let client = crate::cli::fanout::connect(endpoint)?;
    let docker = client.inner();

    let rows: Vec<Value> = match resource {
        ResourceType::Nodes => {
            crate::cli::rows::node_rows(&crate::api::node::list_nodes(docker).await?)
                .iter()
                .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
                .collect()
        }
        ResourceType::Services => crate::cli::rows::service_rows(
            docker,
            &crate::api::service::list_services(docker).await?,
        )
        .await?
        .iter()
        .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
        .collect(),
        ResourceType::Networks => {
            crate::cli::rows::network_rows(&crate::api::network::list_networks(docker).await?)
                .iter()
                .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
                .collect()
        }
        ResourceType::Secrets => {
            crate::cli::rows::secret_rows(&crate::api::secret::list_secrets(docker).await?)
                .iter()
                .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
                .collect()
        }
        ResourceType::Configs => {
            crate::cli::rows::config_rows(&crate::api::config::list_configs(docker).await?)
                .iter()
                .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
                .collect()
        }
        ResourceType::Stacks => {
            crate::cli::rows::stack_rows(&crate::api::stack::list_stacks(docker).await?)
                .iter()
                .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
                .collect()
        }
        ResourceType::Tasks => anyhow::bail!("tasks cannot be compared across clusters"),
    };

    Ok(rows.into_iter().map(|row| (row_key(&row), row)).collect())
}

/// Identity of a row within its resource kind.
///
/// `name` is the human-facing key; `id` is used as a fallback so rows that carry
/// no name (tasks use id) still compare.
fn row_key(row: &Value) -> String {
    for field in ["name", "hostname", "id"] {
        if let Some(Value::String(s)) = row.get(field)
            && !s.is_empty()
        {
            return s.clone();
        }
    }
    String::new()
}

/// Compare two name-keyed row sets.
fn compare(
    left: &BTreeMap<String, Value>,
    right: &BTreeMap<String, Value>,
    kind: &str,
) -> Vec<ResourceDiff> {
    let mut diffs = Vec::new();

    for (name, row) in left {
        match right.get(name) {
            None => diffs.push(ResourceDiff {
                kind: kind.to_string(),
                name: name.clone(),
                change: "removed".to_string(),
                fields: Vec::new(),
            }),
            Some(other) => {
                let fields = field_diffs(row, other);
                if !fields.is_empty() {
                    diffs.push(ResourceDiff {
                        kind: kind.to_string(),
                        name: name.clone(),
                        change: "changed".to_string(),
                        fields,
                    });
                }
            }
        }
    }

    for name in right.keys() {
        if !left.contains_key(name) {
            diffs.push(ResourceDiff {
                kind: kind.to_string(),
                name: name.clone(),
                change: "added".to_string(),
                fields: Vec::new(),
            });
        }
    }

    diffs
}

/// Field-level differences between two row objects.
///
/// Fields present on only one side are reported too, so a schema change between
/// cluster versions shows up rather than being silently ignored.
fn field_diffs(left: &Value, right: &Value) -> Vec<FieldDiff> {
    let mut fields = Vec::new();
    let mut keys: Vec<&String> = left
        .as_object()
        .map(|o| o.keys().collect())
        .unwrap_or_default();
    if let Some(obj) = right.as_object() {
        for k in obj.keys() {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
    }
    keys.sort();

    for key in keys {
        let a = left.get(key);
        let b = right.get(key);
        if a == b {
            continue;
        }
        fields.push(FieldDiff {
            field: key.clone(),
            left: render(a),
            right: render(b),
        });
    }
    fields
}

fn render(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => NONE_CELL.to_string(),
        Some(Value::String(s)) if s.is_empty() => NONE_CELL.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// Human-readable summary of a diff.
pub fn render_diff(diff: &ClusterDiff) -> String {
    let mut out = String::new();

    if let Some(err) = &diff.left_unreachable {
        out.push_str(&format!("{}: unreachable ({})\n", diff.left, err));
    }
    if let Some(err) = &diff.right_unreachable {
        out.push_str(&format!("{}: unreachable ({})\n", diff.right, err));
    }
    if diff.left_unreachable.is_some() || diff.right_unreachable.is_some() {
        return out;
    }

    if diff.identical {
        out.push_str(&format!(
            "{} and {} are identical.\n",
            diff.left, diff.right
        ));
        return out;
    }

    out.push_str(&format!(
        "{} vs {}: {} difference(s)\n",
        diff.left,
        diff.right,
        diff.diff_count()
    ));
    for entry in &diff.diffs {
        let marker = match entry.change.as_str() {
            "added" => "+",
            "removed" => "-",
            _ => "~",
        };
        out.push_str(&format!("  {} {} {}\n", marker, entry.kind, entry.name));
        for field in &entry.fields {
            out.push_str(&format!(
                "      {}: {} -> {}\n",
                field.field, field.left, field.right
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    fn rows(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn identical_clusters_report_nothing() {
        let r = rows(&[("web", json!({"replicas": "3/3"}))]);
        let diffs = compare(&r, &r, "service");
        assert!(diffs.is_empty());
    }

    #[test]
    fn reports_changed_fields() {
        let left = rows(&[("web", json!({"replicas": "3/3", "image": "nginx"}))]);
        let right = rows(&[("web", json!({"replicas": "1/3", "image": "nginx"}))]);
        let diffs = compare(&left, &right, "service");

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].change, "changed");
        assert_eq!(diffs[0].fields.len(), 1);
        assert_eq!(diffs[0].fields[0].field, "replicas");
        assert_eq!(diffs[0].fields[0].left, "3/3");
        assert_eq!(diffs[0].fields[0].right, "1/3");
    }

    #[test]
    fn reports_added_and_removed() {
        let left = rows(&[("web", json!({"image": "nginx"}))]);
        let right = rows(&[("api", json!({"image": "api"}))]);
        let diffs = compare(&left, &right, "service");

        let changes: Vec<(String, String)> = diffs
            .iter()
            .map(|d| (d.name.clone(), d.change.clone()))
            .collect();
        assert!(changes.contains(&("web".to_string(), "removed".to_string())));
        assert!(changes.contains(&("api".to_string(), "added".to_string())));
    }

    #[test]
    fn reports_a_field_present_on_only_one_side() {
        let left = rows(&[("web", json!({"a": "1"}))]);
        let right = rows(&[("web", json!({"a": "1", "b": "2"}))]);
        let diffs = compare(&left, &right, "service");
        assert_eq!(diffs[0].fields.len(), 1);
        assert_eq!(diffs[0].fields[0].field, "b");
        assert_eq!(diffs[0].fields[0].left, NONE_CELL);
        assert_eq!(diffs[0].fields[0].right, "2");
    }

    #[test]
    fn empty_versus_populated_is_all_additions() {
        let left = BTreeMap::new();
        let right = rows(&[("web", json!({"a": "1"})), ("api", json!({"a": "2"}))]);
        let diffs = compare(&left, &right, "service");
        assert_eq!(diffs.len(), 2);
        assert!(diffs.iter().all(|d| d.change == "added"));
    }

    #[test]
    fn row_key_prefers_name_then_hostname_then_id() {
        assert_eq!(row_key(&json!({"name": "web", "id": "x"})), "web");
        assert_eq!(row_key(&json!({"hostname": "node1"})), "node1");
        assert_eq!(row_key(&json!({"id": "abc"})), "abc");
        // An empty name falls through rather than producing an empty key.
        assert_eq!(row_key(&json!({"name": "", "id": "abc"})), "abc");
        assert_eq!(row_key(&json!({})), "");
    }

    #[test]
    fn render_normalises_absent_and_empty_values() {
        assert_eq!(render(None), NONE_CELL);
        assert_eq!(render(Some(&Value::Null)), NONE_CELL);
        assert_eq!(render(Some(&json!(""))), NONE_CELL);
        assert_eq!(render(Some(&json!("x"))), "x");
        assert_eq!(render(Some(&json!(3))), "3");
    }

    #[test]
    fn tasks_are_not_comparable_across_clusters() {
        assert!(!is_comparable(ResourceType::Tasks));
        assert!(is_comparable(ResourceType::Services));
        assert!(is_comparable(ResourceType::Nodes));
    }

    #[test]
    fn render_diff_summarises_changes() {
        let d = ClusterDiff {
            left: "prod-eu".into(),
            right: "prod-us".into(),
            left_unreachable: None,
            right_unreachable: None,
            identical: false,
            diffs: vec![ResourceDiff {
                kind: "service".into(),
                name: "web".into(),
                change: "changed".into(),
                fields: vec![FieldDiff {
                    field: "replicas".into(),
                    left: "3/3".into(),
                    right: "1/3".into(),
                }],
            }],
        };
        let text = render_diff(&d);
        assert!(text.contains("1 difference(s)"), "got: {text}");
        assert!(text.contains("~ service web"), "got: {text}");
        assert!(text.contains("replicas: 3/3 -> 1/3"), "got: {text}");
    }

    #[test]
    fn render_diff_reports_identical_clusters() {
        let d = ClusterDiff {
            left: "a".into(),
            right: "b".into(),
            identical: true,
            ..Default::default()
        };
        assert!(render_diff(&d).contains("identical"));
    }

    #[test]
    fn render_diff_reports_unreachable_ends() {
        let d = ClusterDiff {
            left: "a".into(),
            right: "b".into(),
            left_unreachable: Some("timed out".into()),
            ..Default::default()
        };
        let text = render_diff(&d);
        assert!(text.contains("a: unreachable (timed out)"), "got: {text}");
    }

    #[tokio::test]
    async fn diff_rejects_tasks_before_connecting() {
        let endpoint = Endpoint {
            name: "e".into(),
            host: "tcp://127.0.0.1:1".into(),
            description: None,
            site: None,
            environment: None,
            tags: Vec::new(),
            tls: None,
            default: false,
        };
        let err = diff(&endpoint, &endpoint, ResourceType::Tasks)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot be compared"), "got: {err}");
    }

    /// Both endpoints used to be fetched sequentially with no timeout, so an
    /// unresponsive `ssh://` endpoint hung `diff-clusters` indefinitely — while
    /// the same endpoint is capped under `--endpoints` fan-out.
    #[test]
    fn diff_fetch_is_bounded_and_matches_fan_out() {
        assert_eq!(
            FETCH_TIMEOUT,
            crate::cli::fanout::ENDPOINT_TIMEOUT,
            "the same endpoint should get the same budget in both commands"
        );
    }

    #[tokio::test]
    async fn a_hung_fetch_reports_a_timeout_rather_than_blocking() {
        let slow = async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok::<BTreeMap<String, Value>, anyhow::Error>(BTreeMap::new())
        };

        let err = with_timeout(slow, Duration::from_millis(50))
            .await
            .expect_err("a hung fetch must surface as an error");
        assert!(err.to_string().contains("timed out"), "{err}");
    }

    #[tokio::test]
    async fn a_fast_fetch_returns_its_result() {
        let fast = async { Ok::<BTreeMap<String, Value>, anyhow::Error>(BTreeMap::new()) };
        assert!(with_timeout(fast, Duration::from_secs(5)).await.is_ok());
    }
}
