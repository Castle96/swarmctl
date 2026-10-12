use crate::cli::registry::{self, ResourceType};

/// Fields each row type exposes, for `--sort-by` and `-o custom-columns`.
///
/// The paths here address the **Kubernetes envelope** (see
/// [`crate::cli::kobject`]), which is the same document `--sort-by`,
/// `--field-selector` and `-o jsonpath` all read. That was not true before:
/// `--sort-by` used to address display-row fields while `--field-selector`
/// addressed raw PascalCase Docker keys, so the two flags disagreed about what
/// a "field" was and `explain` could only document one of them.
pub fn fields_for(resource: ResourceType) -> &'static [(&'static str, &'static str)] {
    match resource {
        ResourceType::Services => &[
            ("metadata.name", "Service name"),
            ("metadata.labels", "Service labels (KEY=VAL)"),
            ("spec.replicas", "Desired replica count"),
            ("spec.mode", "Service mode (replicated or global)"),
            ("spec.taskTemplate.containerSpec.image", "Container image"),
            ("dockerId", "Service ID"),
        ],
        ResourceType::Nodes => &[
            ("metadata.name", "Node name"),
            ("metadata.labels", "Node labels (KEY=VAL)"),
            (
                "spec.availability",
                "Node availability (active, pause, drain)",
            ),
            ("status.state", "Node status (ready, down)"),
            ("dockerId", "Node ID"),
        ],
        ResourceType::Tasks => &[
            ("metadata.name", "Task name"),
            ("metadata.labels", "Task labels (KEY=VAL)"),
            ("status.desiredState", "Desired state (running, shutdown)"),
            ("status.state", "Current state"),
            ("spec.containerSpec.image", "Container image"),
            ("dockerId", "Task ID"),
        ],
        ResourceType::Networks => &[
            ("metadata.name", "Network name"),
            ("metadata.labels", "Network labels (KEY=VAL)"),
            ("spec.driver", "Network driver (overlay, bridge)"),
            ("spec.scope", "Network scope (swarm, local)"),
            ("spec.internal", "Whether the network is internal"),
            ("dockerId", "Network ID"),
        ],
        ResourceType::Secrets => &[
            ("metadata.name", "Secret name"),
            ("metadata.labels", "Secret labels (KEY=VAL)"),
            ("metadata.creationTimestamp", "Creation timestamp"),
            ("dockerId", "Secret ID"),
        ],
        ResourceType::Configs => &[
            ("metadata.name", "Config name"),
            ("metadata.labels", "Config labels (KEY=VAL)"),
            ("metadata.creationTimestamp", "Creation timestamp"),
            ("dockerId", "Config ID"),
        ],
        ResourceType::Stacks => &[
            ("metadata.name", "Stack name"),
            ("spec.services", "Number of services"),
            ("spec.replicas", "Total replicas"),
        ],
    }
}

pub async fn run(resource: Option<String>) -> anyhow::Result<()> {
    let Some(token) = resource.filter(|r| r != "help") else {
        return print_index();
    };

    let resolved = registry::resolve(&token)?;
    let info = registry::info(resolved);
    let mut width = info.kind.len();

    println!("{} ({} {})", info.kind, info.plural, info.kind);
    println!("{}", "-".repeat(72));
    println!("  apiVersion: {}", crate::cli::kobject::api_version());
    println!("  shortNames: {}", registry::short_names_joined(info));
    println!("  namespaced: {}", info.namespaced);
    println!();
    println!("FIELDS (usable with --sort-by, --field-selector, -o jsonpath,");
    println!("-o custom-columns and -L)");

    let fields = fields_for(resolved);
    width = width
        .max(6)
        .max(fields.iter().map(|(p, _)| p.len()).max().unwrap_or(0));
    let pad = width + 2;
    for (path, doc) in fields {
        println!("  {:<pad$}  {}", path, doc, pad = pad);
    }

    println!();
    println!("SELECTOR EXAMPLES");
    println!("  --selector app=web                     equality");
    println!("  --selector 'env in (prod,stage)'       set membership");
    println!("  --selector '!legacy'                   key is absent");
    println!("  --field-selector status.state=ready    field equality");
    println!("  --sort-by '{{.metadata.name}}'          ordering");
    println!("  -o jsonpath='{{.metadata.name}}'        templated output");
    println!("  -o custom-columns=NAME:.metadata.name  arbitrary columns");

    if resolved == ResourceType::Stacks {
        println!();
        println!("Stacks are synthesized from services rather than fetched, so");
        println!("they have no labels or API object: --selector and");
        println!("--field-selector do not apply to them.");
    }

    Ok(())
}

fn print_index() -> anyhow::Result<()> {
    println!("Resource Types:");
    for info in registry::REGISTRY {
        let aliases = registry::all_spellings_for(info);
        println!(
            "  {:<10} {:<12} - Docker Swarm {}",
            info.plural,
            format!("({})", aliases.join(", ")),
            info.kind
        );
    }
    println!();
    println!("Use 'swarmctl explain <resource>' for field details.");
    println!();
    println!("'swarmctl api-resources' prints the same table as a resource listing.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::registry;

    /// Every registry entry must have documentation, or `explain <kind>` fails
    /// for a resource `get <kind>` handles fine.
    #[test]
    fn every_resource_is_documented() {
        for info in registry::REGISTRY {
            assert!(
                !fields_for(info.resource).is_empty(),
                "{} has no documented fields",
                info.plural
            );
        }
    }

    /// Documented paths must actually resolve against the serialized envelope.
    ///
    /// This is the test that stops `explain` from drifting: it used to document
    /// display-row fields while the selector machinery addressed something
    /// else entirely.
    #[test]
    fn documented_paths_exist_on_a_sample_object() {
        let samples: &[(ResourceType, serde_json::Value)] = &[
            (
                ResourceType::Services,
                serde_json::json!({
                    "ID": "id1",
                    "CreatedAt": "2026-01-01T00:00:00Z",
                    "Spec": {
                        "Name": "web",
                        "Labels": {"app": "web"},
                        "Mode": {"Replicated": {"Replicas": 2}},
                        "TaskTemplate": {"ContainerSpec": {"Image": "nginx"}}
                    }
                }),
            ),
            (
                ResourceType::Nodes,
                serde_json::json!({
                    "ID": "n1",
                    "Spec": {"Name": "node1", "Availability": "ACTIVE",
                             "Labels": {"role": "worker"}},
                    "Status": {"State": "READY"}
                }),
            ),
            (
                ResourceType::Tasks,
                serde_json::json!({
                    "ID": "t1",
                    "Name": "web.1",
                    "DesiredState": "running",
                    "Spec": {"ContainerSpec": {"Image": "nginx", "Labels": {"app": "web"}}},
                    "Status": {"State": "running"}
                }),
            ),
            (
                ResourceType::Networks,
                serde_json::json!({"ID": "nw1", "Labels": {"app": "web"},
                    "Spec": {"Name": "net", "Driver": "overlay", "Scope": "swarm", "Internal": false}}),
            ),
            (
                ResourceType::Secrets,
                serde_json::json!({"ID": "s1", "CreatedAt": "2026-01-01T00:00:00Z",
                    "Spec": {"Name": "sec", "Labels": {"app": "web"}}}),
            ),
            (
                ResourceType::Configs,
                serde_json::json!({"ID": "c1", "CreatedAt": "2026-01-01T00:00:00Z",
                    "Spec": {"Name": "cfg", "Labels": {"app": "web"}}}),
            ),
        ];

        for (resource, raw) in samples {
            let envelope = crate::cli::kobject::to_k8s(*resource, raw);
            for (path, _) in fields_for(*resource) {
                let found = crate::utils::jsonpath::parse(path)
                    .expect("documented path must parse")
                    .eval(&envelope)
                    .expect("documented path must evaluate");
                assert!(
                    !found.is_empty(),
                    "explain documents {path} for {} but it resolves to nothing",
                    registry::info(*resource).plural
                );
            }
        }
    }
}
