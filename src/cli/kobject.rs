//! Kubernetes-shaped serialization for Docker Swarm objects.
//!
//! `-o json` used to emit bollard's structs verbatim, which means PascalCase keys
//! and no envelope at all:
//!
//! ```json
//! {"ID":"n1","Spec":{"Name":"web","Availability":"active"},"Status":{"State":"ready"}}
//! ```
//!
//! That is why `metadata.name` was unreachable, why `--field-selector` had to be
//! written `Spec.Name=web`, and why `apply.rs` probed for both `"Name"` and
//! `"name"`. Everything here exists to make one canonical shape:
//!
//! ```json
//! {
//!   "apiVersion": "v1.52",
//!   "kind": "Service",
//!   "metadata": {"name": "web", "labels": {"app": "web"}},
//!   "spec":   {"taskTemplate": {"containerSpec": {"image": "nginx"}}},
//!   "status": {...}
//! }
//! ```
//!
//! The mapping is deliberately **generic**: it reads the serialized Docker
//! object and pulls out the fields every Swarm resource happens to share, so a
//! resource nobody anticipated still gets `metadata.name`. Per-resource quirks
//! are handled by [`name_of`], which knows where Swarm stashes the human name.

use crate::cli::registry::{self, ResourceType};
use serde_json::{Map, Value};

/// The API version reported in `apiVersion`.
pub fn api_version() -> String {
    format!("v{}", bollard::API_DEFAULT_VERSION)
}

/// Wrap a raw Docker object in the Kubernetes envelope.
///
/// The raw object is expected to be the serialized `bollard::models::*` value
/// (PascalCase). Keys are rewritten to camelCase and the well-known Swarm
/// metadata is hoisted into `metadata`.
pub fn to_k8s(resource: ResourceType, raw: &Value) -> Value {
    let info = registry::info(resource);

    let mut out = Map::new();
    out.insert("apiVersion".into(), Value::String(api_version()));
    out.insert("kind".into(), Value::String(info.kind.into()));

    // `metadata` is always present, even for objects with no Docker identity
    // field, so `{.metadata.name}` never errors on a well-formed path.
    //
    // Metadata is read from the **raw** object: Docker spells these keys
    // `Spec.Name`, and camelCasing first would hide them behind `spec.name`.
    out.insert("metadata".into(), metadata_for(resource, raw));

    // Only the body is camelCased, so `spec.taskTemplate` is reachable while
    // the metadata extraction above still finds the Go-style keys.
    let spec = to_camel_case(raw.get("Spec").unwrap_or(&Value::Null));
    out.insert("spec".into(), normalize_spec(resource, spec));

    let mut status = raw
        .get("Status")
        .map(|v| normalize_status(resource, to_camel_case(v)))
        .unwrap_or(Value::Object(Map::new()));
    // A task's desired state is a top-level Docker field, but it is state, so
    // fold it into `status` where callers expect to find it.
    if let (Some(desired), Value::Object(map)) = (raw.get("DesiredState"), &mut status) {
        map.insert("desiredState".into(), desired.clone());
    }
    out.insert("status".into(), status);

    // Docker's own identity survives under a distinct key rather than being lost.
    if let Some(id) = raw.get("ID") {
        out.insert("dockerId".into(), id.clone());
    }

    Value::Object(out)
}

/// Wrap a collection as a Kubernetes-style list.
///
/// `kubectl get -o json` returns an envelope with `items`, not a bare array,
/// which is what lets `jq` and jsonpath be written against one shape.
pub fn to_k8s_list(resource: ResourceType, items: &[Value]) -> Value {
    let info = registry::info(resource);
    let out: Vec<Value> = items.iter().map(|i| to_k8s(resource, i)).collect();
    let mut root = Map::new();
    root.insert("apiVersion".into(), Value::String(api_version()));
    root.insert("kind".into(), Value::String(format!("{}List", info.kind)));
    root.insert("items".into(), Value::Array(out));
    Value::Object(root)
}

/// Build `metadata` from wherever Swarm keeps the object's identity.
fn metadata_for(resource: ResourceType, obj: &Value) -> Value {
    let mut meta = Map::new();

    let name = name_of(resource, obj).unwrap_or_default();
    meta.insert("name".into(), Value::String(name));

    let labels = labels_of(resource, obj).filter(|l| l.as_object().is_some_and(|m| !m.is_empty()));
    if let Some(labels) = labels {
        meta.insert("labels".into(), labels);
    }
    if let Some(created) = first_string(obj, &["CreatedAt", "createdAt", "created"]) {
        meta.insert("creationTimestamp".into(), Value::String(created));
    }
    if let Some(id) = obj.get("ID").and_then(Value::as_str) {
        meta.insert("uid".into(), Value::String(id.to_string()));
    }
    // Swarm's stack membership is a label, and it is the closest thing to a
    // namespace that Swarm has, so it is surfaced where callers expect one.
    if let Some(stack) = stack_namespace_of(resource, obj) {
        meta.insert("stack".into(), Value::String(stack));
    }

    Value::Object(meta)
}

/// The human-facing name, which Swarm does not store consistently.
pub fn name_of(resource: ResourceType, obj: &Value) -> Option<String> {
    let spec = obj.get("Spec").unwrap_or(obj);
    let candidate = match resource {
        ResourceType::Services => spec.get("Name").and_then(Value::as_str).map(str::to_string),
        // A Swarm node's `Spec.Name` is usually empty; the operator-facing name
        // lives in `Description.Hostname`. Without this, `metadata.name` for
        // every node was the 25-char node ID, which is not what `get nodes`
        // prints in its HOSTNAME column.
        ResourceType::Nodes => obj
            .get("Description")
            .and_then(|d| d.get("Hostname"))
            .or_else(|| spec.get("Name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        ResourceType::Tasks => obj.get("Name").and_then(Value::as_str).map(str::to_string),
        // A task's own `Name` is its container name, which is noisy and unique
        // per run; the service it belongs to is what a user means by "name".
        ResourceType::Stacks => spec.get("Name").and_then(Value::as_str).map(str::to_string),
        _ => spec.get("Name").and_then(Value::as_str).map(str::to_string),
    };
    if candidate.is_some() {
        return candidate;
    }
    // Tasks fall back to the service name they were created for.
    if resource == ResourceType::Tasks
        && let Some(svc) = obj.get("ServiceID").and_then(Value::as_str)
    {
        // Service ids look like a 25-char base36 token; it is the best name
        // available when the API did not populate one.
        return Some(svc.to_string());
    }
    obj.get("ID").and_then(Value::as_str).map(str::to_string)
}

/// The label map, whose location differs per resource.
pub fn labels_of(resource: ResourceType, obj: &Value) -> Option<Value> {
    let spec = obj.get("Spec").unwrap_or(obj);
    let direct = spec.get("Labels").or_else(|| obj.get("Labels"));
    if let Some(v) = direct {
        return Some(v.clone());
    }
    if resource == ResourceType::Tasks
        && let Some(cs) = spec.get("ContainerSpec").and_then(|c| c.get("Labels"))
    {
        return Some(cs.clone());
    }
    None
}

/// The `com.docker.stack.namespace` label, which is Swarm's grouping mechanism.
fn stack_namespace_of(resource: ResourceType, obj: &Value) -> Option<String> {
    if resource != ResourceType::Services {
        return None;
    }
    let labels = labels_of(resource, obj)?;
    labels
        .get("com.docker.stack.namespace")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn first_string(obj: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = obj.get(*k).and_then(Value::as_str) {
            return Some(s.to_string());
        }
    }
    None
}

/// Rename every object key from PascalCase to camelCase, recursively.
///
/// Docker's stubs carry explicit `#[serde(rename = "TaskTemplate")]` attributes,
/// so the wire format is Go-style. Kubernetes is camelCase, and users will type
/// `{.spec.taskTemplate.containerSpec.image}`.
pub fn to_camel_case(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, child) in map {
                out.insert(to_camel_key(k), to_camel_case(child));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(to_camel_case).collect()),
        other => other.clone(),
    }
}

/// Lowercase the leading acronym of a PascalCase key.
///
/// `ID` becomes `id`, not `iD`, and `TaskTemplate` becomes `taskTemplate`.
fn to_camel_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let uppercase_run = chars.iter().take_while(|c| c.is_uppercase()).count();
    let mut out = String::with_capacity(key.len());
    if uppercase_run <= 1 {
        // `TaskTemplate`: only the first character changes.
        for (i, c) in chars.iter().enumerate() {
            if i == 0 {
                out.extend(c.to_lowercase());
            } else {
                out.push(*c);
            }
        }
    } else if uppercase_run == chars.len() {
        // `ID`: an all-caps acronym lowercases wholesale.
        out.extend(chars.iter().flat_map(|c| c.to_lowercase()));
    } else {
        // `IDName`: lowercase the acronym but keep the boundary capital.
        for (i, c) in chars.iter().enumerate() {
            if i + 1 < uppercase_run {
                out.extend(c.to_lowercase());
            } else {
                out.push(*c);
            }
        }
    }
    out
}

/// Per-resource spec cleanups after camelCasing.
fn normalize_spec(resource: ResourceType, spec: Value) -> Value {
    // `id` inside a spec is a Swarm implementation detail (e.g. a container
    // id); `dockerId` at the top level already carries identity, so drop it.
    let mut spec = spec;
    if let Value::Object(map) = &mut spec {
        map.remove("id");
    }
    if resource == ResourceType::Services
        && let Some(replicas) = replicas_from_mode(&spec)
        && let Value::Object(map) = &mut spec
    {
        map.insert("replicas".into(), replicas);
    }
    spec
}

/// Swarm encodes replicated/global mode as a one-of, so `spec.replicas` has to
/// be pulled out of `spec.mode` for anything to select on it.
///
/// Keys are camelCase because this runs *after* [`to_camel_case`].
fn replicas_from_mode(spec: &Value) -> Option<Value> {
    let mode = spec.get("mode")?.get("replicated")?;
    mode.get("replicas").cloned()
}

/// Per-resource status cleanups.
///
/// Node state and availability are upper-case in Docker (`READY`, `ACTIVE`) and
/// lower-case everywhere else in swarmctl's own output; normalizing here means
/// `--field-selector status.state=ready` matches what the table prints.
fn normalize_status(resource: ResourceType, status: Value) -> Value {
    let mut status = status;
    if let Value::Object(map) = &mut status {
        for key in ["state", "availability", "reachability"] {
            if let Some(Value::String(s)) = map.get(key)
                && s.chars().all(|c| !c.is_lowercase())
            {
                let lowered = s.to_lowercase();
                map.insert(key.to_string(), Value::String(lowered));
            }
        }
    }
    let _ = resource;
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn service() -> Value {
        json!({
            "ID": "svcab123",
            "Version": {"Index": 42},
            "CreatedAt": "2026-01-02T03:04:05Z",
            "Spec": {
                "Name": "web",
                "Labels": {"app": "web", "com.docker.stack.namespace": "prod"},
                "Mode": {"Replicated": {"Replicas": 3}},
                "TaskTemplate": {
                    "ContainerSpec": {"Image": "nginx:1.25"}
                }
            },
            "Endpoint": {"Ports": [{"PublishedPort": 8080}]}
        })
    }

    #[test]
    fn envelope_carries_api_version_and_kind() {
        let out = to_k8s(ResourceType::Services, &service());
        assert_eq!(out["apiVersion"], json!(api_version()));
        assert_eq!(out["kind"], json!("Service"));
        assert!(out["metadata"].is_object());
        assert!(out["spec"].is_object());
    }

    #[test]
    fn metadata_name_comes_from_spec_name() {
        let out = to_k8s(ResourceType::Services, &service());
        assert_eq!(out["metadata"]["name"], json!("web"));
        assert_eq!(out["metadata"]["labels"]["app"], json!("web"));
        assert_eq!(out["metadata"]["stack"], json!("prod"));
        assert_eq!(out["metadata"]["uid"], json!("svcab123"));
    }

    #[test]
    fn spec_keys_are_camel_cased() {
        let out = to_k8s(ResourceType::Services, &service());
        assert_eq!(
            out["spec"]["taskTemplate"]["containerSpec"]["image"],
            json!("nginx:1.25")
        );
        // The PascalCase spelling is gone, so nothing silently returns null.
        assert!(out["spec"].get("TaskTemplate").is_none());
    }

    #[test]
    fn replicas_are_lifted_out_of_the_mode_one_of() {
        let out = to_k8s(ResourceType::Services, &service());
        assert_eq!(out["spec"]["replicas"], json!(3));
    }

    #[test]
    fn docker_id_survives_under_its_own_key() {
        let out = to_k8s(ResourceType::Services, &service());
        assert_eq!(out["dockerId"], json!("svcab123"));
    }

    #[test]
    fn list_wraps_items_in_an_envelope() {
        let list = to_k8s_list(ResourceType::Services, &[service(), service()]);
        assert_eq!(list["kind"], json!("ServiceList"));
        assert_eq!(list["items"].as_array().unwrap().len(), 2);
        assert_eq!(list["items"][0]["kind"], json!("Service"));
    }

    /// Every kind must produce the same envelope shape, even when the object
    /// carries no name at all, so one jsonpath works across all of them.
    #[test]
    fn every_resource_kind_produces_the_same_envelope_shape() {
        let named = json!({"ID": "x1", "Spec": {"Name": "thing"}});
        for info in registry::REGISTRY {
            let out = to_k8s(info.resource, &named);
            assert_eq!(out["kind"], json!(info.kind));
            assert_eq!(out["apiVersion"], json!(api_version()));
            assert!(out["metadata"].is_object(), "{} lacks metadata", info.kind);
            assert!(out["spec"].is_object(), "{} lacks spec", info.kind);

            let bare = to_k8s(info.resource, &json!({"Spec": {"Driver": "overlay"}}));
            assert_eq!(
                bare["metadata"]["name"],
                json!(""),
                "{} should still carry an empty name",
                info.kind
            );
        }
    }

    /// A task's human name lives at the top level, not under `Spec`.
    #[test]
    fn task_name_comes_from_the_top_level_field() {
        let raw = json!({"ID": "t1", "Name": "web.1.abc", "Spec": {"Name": "ignored"}});
        assert_eq!(name_of(ResourceType::Tasks, &raw).unwrap(), "web.1.abc");
    }

    #[test]
    fn camel_key_conversion_leaves_values_alone() {
        assert_eq!(to_camel_key("TaskTemplate"), "taskTemplate");
        assert_eq!(to_camel_key("ID"), "id");
        assert_eq!(to_camel_key(""), "");
        assert_eq!(to_camel_key("Already"), "already");
    }

    #[test]
    fn node_status_is_lowercased_to_match_the_table() {
        let raw = json!({"Spec": {"Name": "n1"},
                         "Status": {"State": "READY"},
                         "Spec2": {"Availability": "ACTIVE"}});
        let out = to_k8s(ResourceType::Nodes, &raw);
        assert_eq!(out["status"]["state"], json!("ready"));
    }

    #[test]
    fn jsonpath_reaches_through_the_envelope() {
        let out = to_k8s(ResourceType::Services, &service());
        let text = crate::utils::jsonpath::eval_text("{.metadata.name}", &out).unwrap();
        assert_eq!(text, "web");
        let image =
            crate::utils::jsonpath::eval_text("{.spec.taskTemplate.containerSpec.image}", &out)
                .unwrap();
        assert_eq!(image, "nginx:1.25");
    }
}
