//! Structured, field-aware comparison of a Docker resource against a manifest.
//!
//! The previous implementation shelled out to `diff -u` on fully serialized
//! Bollard objects. Live objects carry server-assigned noise (`Version`,
//! `CreatedAt`, `UpdateStatus`, node bookkeeping) that no manifest can
//! reproduce, so every diff showed a wall of false positives that buried the
//! real change. This module compares the *desired* fields only, from an
//! explicit ignore list, so one differing line means one real difference.
//!
//! Pure logic throughout: it takes two [`serde_json::Value`] objects and
//! returns a [`SpecDiff`], so it is fully unit-testable with no daemon.

use serde_json::Value;
use std::collections::BTreeMap;

/// Server-assigned fields that no manifest reproduces.
///
/// Compared by *last* path segment so the same rule applies at any nesting
/// depth, and applied to both sides of the comparison.
pub const IGNORED_FIELDS: &[&str] = &[
    "Version",
    "CreatedAt",
    "Created",
    "UpdatedAt",
    "UpdateStatus",
    "ServiceStatus",
    "VirtualIPs",
    "AssignedGenericResources",
    "DisruptiveUpdates",
    "JobStatus",
    // Bumped by the scheduler on every update; a manifest never sets it, and
    // `rollout restart` increments it deliberately.
    "ForceUpdate",
];

/// One differing leaf in a resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    /// Dot-separated path to the leaf, e.g. `Spec.TaskTemplate.ContainerSpec.Image`.
    pub path: String,
    /// The live (server-side) value, rendered compactly.
    pub live: String,
    /// The manifest value, rendered compactly.
    pub desired: String,
}

/// Field-level differences for one resource.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecDiff {
    /// Resource name the diff belongs to.
    pub name: String,
    /// Every differing leaf, sorted by path.
    pub changes: Vec<FieldChange>,
}

/// Render a JSON value for a one-line diff cell.
///
/// Objects and arrays collapse to compact JSON; strings print bare so the
/// common case (`nginx:1.25`) reads as itself rather than `"nginx:1.25"`.
pub fn render_value(value: &Value) -> String {
    match value {
        Value::Null => "<absent>".to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Whether a path segment is a server-assigned field that must not be diffed.
pub fn is_ignored(segment: &str) -> bool {
    IGNORED_FIELDS.contains(&segment)
}

/// Compare two objects and report the differing leaves.
///
/// The two arguments are independent of order: `live` fills the `live` column
/// and `desired` the `desired` column. A field present on only one side is
/// reported as a change, so a manifest field the cluster never materialised
/// is visible rather than silently ignored.
pub fn compare_objects(live: &Value, desired: &Value) -> Vec<FieldChange> {
    let mut changes = Vec::new();
    walk(live, desired, "", &mut changes);
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

/// Recursively compare, collecting leaf differences.
///
/// Objects recurse key-by-key; every other value compares directly. Container
/// length is ignored because it is derived from the children.
fn walk(live: &Value, desired: &Value, prefix: &str, out: &mut Vec<FieldChange>) {
    match (live, desired) {
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                if is_ignored(key) {
                    continue;
                }
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                let lv = a.get(key).unwrap_or(&Value::Null);
                let dv = b.get(key).unwrap_or(&Value::Null);
                walk(lv, dv, &path, out);
            }
        }
        (l, d) if l == d => {}
        (l, d) => out.push(FieldChange {
            path: prefix.to_string(),
            live: render_value(l),
            desired: render_value(d),
        }),
    }
}

/// Compare a set of live objects against a set of desired objects, keyed by
/// resource name.
///
/// Reports three kinds of difference:
/// - `changed` — present on both sides with differing fields
/// - `added` — desired but absent from the cluster
/// - `removed` — present on the cluster but not in the manifest
pub fn compare_sets(
    live: &BTreeMap<String, Value>,
    desired: &BTreeMap<String, Value>,
) -> Vec<SpecDiff> {
    let mut diffs = Vec::new();

    for (name, wanted) in desired {
        match live.get(name) {
            None => diffs.push(SpecDiff {
                name: name.clone(),
                changes: vec![FieldChange {
                    path: "<resource>".to_string(),
                    live: "<absent>".to_string(),
                    desired: "present".to_string(),
                }],
            }),
            Some(current) => {
                let changes = compare_objects(current, wanted);
                if !changes.is_empty() {
                    diffs.push(SpecDiff {
                        name: name.clone(),
                        changes,
                    });
                }
            }
        }
    }

    for name in live.keys() {
        if !desired.contains_key(name) {
            diffs.push(SpecDiff {
                name: name.clone(),
                changes: vec![FieldChange {
                    path: "<resource>".to_string(),
                    live: "present".to_string(),
                    desired: "<absent>".to_string(),
                }],
            });
        }
    }

    diffs.sort_by(|a, b| a.name.cmp(&b.name));
    diffs
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identical_objects_report_no_changes() {
        let a = json!({"Image": "nginx:1.25", "Name": "web"});
        assert!(compare_objects(&a, &a).is_empty());
    }

    #[test]
    fn reports_a_differing_scalar_with_dotted_path() {
        let live = json!({"Spec": {"Image": "nginx:1.25"}});
        let desired = json!({"Spec": {"Image": "nginx:1.26"}});
        let changes = compare_objects(&live, &desired);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "Spec.Image");
        assert_eq!(changes[0].live, "nginx:1.25");
        assert_eq!(changes[0].desired, "nginx:1.26");
    }

    #[test]
    fn server_assigned_version_fields_are_ignored() {
        // These three are exactly what made the old `diff -u` output useless.
        let live = json!({
            "Version": {"Index": 42},
            "CreatedAt": "2026-01-01T00:00:00Z",
            "Spec": {"Image": "nginx:1.25"}
        });
        let desired = json!({"Spec": {"Image": "nginx:1.25"}});
        assert!(
            compare_objects(&live, &desired).is_empty(),
            "server-assigned fields must not produce diffs"
        );
    }

    #[test]
    fn ignored_fields_are_skipped_at_any_depth() {
        let live = json!({"a": {"b": {"Version": {"Index": 9}, "x": 1}}});
        let desired = json!({"a": {"b": {"Version": {"Index": 99}, "x": 1}}});
        assert!(compare_objects(&live, &desired).is_empty());
    }

    #[test]
    fn a_field_missing_from_the_desired_side_is_reported() {
        let live = json!({"a": 1, "b": 2});
        let desired = json!({"a": 1});
        let changes = compare_objects(&live, &desired);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "b");
        assert_eq!(changes[0].live, "2");
        assert_eq!(changes[0].desired, "<absent>");
    }

    #[test]
    fn container_length_does_not_matter() {
        let live = json!({"Ports": [{"a": 1}]});
        let desired = json!({"Ports": []});
        // The array elements differ, but not because of length alone.
        let changes = compare_objects(&live, &desired);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "Ports");
    }

    #[test]
    fn empty_arrays_and_objects_compare_equal() {
        let live = json!({"Networks": [], "Labels": {}});
        let desired = json!({"Networks": [], "Labels": {}});
        assert!(compare_objects(&live, &desired).is_empty());
    }

    #[test]
    fn numeric_difference_is_reported_as_strings() {
        let live = json!({"Replicas": 3});
        let desired = json!({"Replicas": 4});
        let changes = compare_objects(&live, &desired);
        assert_eq!(changes[0].live, "3");
        assert_eq!(changes[0].desired, "4");
    }

    #[test]
    fn changes_are_sorted_by_path() {
        let live = json!({"z": 1, "a": 1});
        let desired = json!({"z": 2, "a": 2});
        let changes = compare_objects(&live, &desired);
        assert_eq!(changes[0].path, "a");
        assert_eq!(changes[1].path, "z");
    }

    #[test]
    fn compare_sets_reports_changed_resources() {
        let mut live = BTreeMap::new();
        live.insert("web".to_string(), json!({"Image": "nginx:1.25"}));
        let mut desired = BTreeMap::new();
        desired.insert("web".to_string(), json!({"Image": "nginx:1.26"}));
        let diffs = compare_sets(&live, &desired);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].name, "web");
        assert_eq!(diffs[0].changes[0].path, "Image");
    }

    #[test]
    fn compare_sets_reports_missing_and_extra_resources() {
        let mut live = BTreeMap::new();
        live.insert("legacy".to_string(), json!({"Image": "old"}));
        let mut desired = BTreeMap::new();
        desired.insert("web".to_string(), json!({"Image": "nginx"}));
        let diffs = compare_sets(&live, &desired);
        assert_eq!(diffs.len(), 2);
        let web = diffs.iter().find(|d| d.name == "web").unwrap();
        let legacy = diffs.iter().find(|d| d.name == "legacy").unwrap();
        assert_eq!(web.changes[0].desired, "present");
        assert_eq!(legacy.changes[0].live, "present");
    }

    #[test]
    fn compare_sets_of_identical_sets_is_empty() {
        let mut live = BTreeMap::new();
        live.insert("web".to_string(), json!({"Image": "nginx"}));
        let desired = live.clone();
        assert!(compare_sets(&live, &desired).is_empty());
    }

    #[test]
    fn render_value_prints_strings_barely() {
        assert_eq!(render_value(&json!("nginx")), "nginx");
        assert_eq!(render_value(&json!(3)), "3");
        assert_eq!(render_value(&Value::Null), "<absent>");
    }

    #[test]
    fn is_ignored_matches_the_constant_list() {
        assert!(is_ignored("Version"));
        assert!(is_ignored("CreatedAt"));
        assert!(!is_ignored("Image"));
    }
}
