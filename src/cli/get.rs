use crate::api::client::DockerClient;
use crate::cli::root::{OutputFormat, ResourceType};
use crate::models::{node::NodeRow, service::ServiceRow};
use crate::utils::printer::{print_json, print_table, print_yaml, render_rows, sort_rows};
use crate::utils::selectors::{format_labels, matches_selector};

/// Flags that shape how a resource list is queried and rendered.
///
/// Bundled into a single struct because every list handler takes the same set.
#[derive(Clone, Default)]
pub struct ListOptions {
    pub show_labels: bool,
    pub selector: Option<String>,
    pub field_selector: Option<String>,
    pub sort_by: Option<String>,
    pub watch: bool,
}

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: Option<String>,
    output: OutputFormat,
    opts: ListOptions,
) -> anyhow::Result<()> {
    let ListOptions {
        show_labels,
        selector,
        field_selector,
        sort_by,
        watch,
    } = opts;
    loop {
        let list_opts = ListOptions {
            show_labels,
            selector: selector.clone(),
            field_selector: field_selector.clone(),
            sort_by: sort_by.clone(),
            watch,
        };
        match resource {
            ResourceType::Nodes => {
                if let Some(name) = &name {
                    get_node(client, name, output).await?;
                } else {
                    get_nodes(client, output, &list_opts).await?;
                }
            }
            ResourceType::Services => {
                if let Some(name) = &name {
                    get_service(client, name, output).await?;
                } else {
                    get_services(client, output, &list_opts).await?;
                }
            }
            ResourceType::Tasks => {
                if let Some(name) = &name {
                    get_task(client, name, output).await?;
                } else {
                    get_tasks(client, output, &list_opts).await?;
                }
            }
            ResourceType::Networks => {
                if let Some(name) = &name {
                    get_network(client, name, output).await?;
                } else {
                    get_networks(client, output, &list_opts).await?;
                }
            }
            ResourceType::Secrets => {
                if let Some(name) = &name {
                    get_secret(client, name, output).await?;
                } else {
                    get_secrets(client, output, &list_opts).await?;
                }
            }
            ResourceType::Configs => {
                if let Some(name) = &name {
                    get_config(client, name, output).await?;
                } else {
                    get_configs(client, output, &list_opts).await?;
                }
            }
            ResourceType::Stacks => {
                if let Some(name) = &name {
                    get_stack(client, name, output).await?;
                } else {
                    get_stacks(client, output, show_labels, watch).await?;
                }
            }
        }

        if !watch {
            break;
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        // Clear after the pause, immediately before redrawing. Clearing here
        // meant the table was visible for microseconds and the screen was then
        // blank for two seconds, which reads as a flicker.
        print!("\x1B[2J\x1B[1;1H");
    }

    Ok(())
}

/// Keep objects matching the label selector and the field selector.
///
/// Selectors are evaluated against the raw API object (not the rendered row),
/// which is what users expect from `--field-selector spec.name=web`.
fn keep<T: serde::Serialize>(
    items: Vec<T>,
    labels: impl Fn(&T) -> Option<std::collections::HashMap<String, String>>,
    selector: Option<&String>,
    field_selector: Option<&String>,
) -> Vec<T> {
    items
        .into_iter()
        .filter(|item| {
            selector
                .map(|sel| matches_selector(&labels(item), sel))
                .unwrap_or(true)
        })
        .filter(|item| {
            field_selector
                .map(|sel| {
                    let json = serde_json::to_value(item).unwrap_or_default();
                    matches_field_selector(&json, sel)
                })
                .unwrap_or(true)
        })
        .collect()
}

fn matches_field_selector(value: &serde_json::Value, field_selector: &str) -> bool {
    let Some((key, expected)) = field_selector.split_once('=') else {
        return true;
    };
    let actual = value.pointer(&crate::utils::printer::json_pointer(key));
    match actual {
        Some(serde_json::Value::String(s)) => s == expected,
        Some(serde_json::Value::Number(n)) => n.to_string() == expected,
        Some(serde_json::Value::Bool(b)) => b.to_string() == expected,
        _ => false,
    }
}

async fn get_nodes(
    client: &DockerClient,
    output: OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    let ListOptions {
        show_labels,
        selector,
        field_selector,
        sort_by,
        ..
    } = opts;
    let show_labels = *show_labels;
    let nodes = crate::api::node::list_nodes(client.inner()).await?;

    let nodes = keep(
        nodes,
        |n: &bollard::models::Node| n.spec.as_ref().and_then(|s| s.labels.clone()),
        selector.as_ref(),
        field_selector.as_ref(),
    );
    let rows: Vec<NodeRow> = crate::cli::rows::node_rows(&nodes);

    let mut rows = rows;
    if let Some(sort_field) = sort_by.as_deref() {
        sort_rows(&mut rows, sort_field)?;
    }
    render_rows(&rows, &output, show_labels)?;

    Ok(())
}

async fn get_services(
    client: &DockerClient,
    output: OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    let ListOptions {
        show_labels,
        selector,
        field_selector,
        sort_by,
        ..
    } = opts;
    let show_labels = *show_labels;
    let services = crate::api::service::list_services(client.inner()).await?;

    let services = keep(
        services,
        |s: &bollard::models::Service| s.spec.as_ref().and_then(|sp| sp.labels.clone()),
        selector.as_ref(),
        field_selector.as_ref(),
    );
    let rows: Vec<ServiceRow> = crate::cli::rows::service_rows(client.inner(), &services).await?;

    let mut rows = rows;
    if let Some(sort_field) = sort_by.as_deref() {
        sort_rows(&mut rows, sort_field)?;
    }
    render_rows(&rows, &output, show_labels)?;

    Ok(())
}

async fn get_node(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;
    let node = nodes
        .into_iter()
        .find(|n| n.spec.as_ref().and_then(|s| s.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Node {} not found", name))?;

    match output {
        OutputFormat::Table | OutputFormat::Wide => {
            let spec = node.spec.as_ref().cloned().unwrap_or_default();
            let status = node.status.unwrap_or_default();
            let hostname = node
                .description
                .as_ref()
                .and_then(|d| d.hostname.clone())
                .or_else(|| spec.name.clone())
                .unwrap_or_default();
            println!("Name: {}", hostname);
            println!("ID: {}", node.id.unwrap_or_default());
            println!(
                "Status: {}",
                status.state.unwrap_or(bollard::models::NodeState::READY)
            );
            println!(
                "Availability: {}",
                spec.availability
                    .unwrap_or(bollard::models::NodeSpecAvailabilityEnum::ACTIVE)
            );
            if let Some(labels) = &spec.labels
                && !labels.is_empty()
            {
                println!("Labels: {}", format_labels(&Some(labels.clone())));
            }
            if let Some(manager_status) = &node.manager_status {
                println!(
                    "Manager Status: {}",
                    manager_status
                        .reachability
                        .unwrap_or(bollard::models::Reachability::UNKNOWN)
                );
            }
        }
        OutputFormat::Json => print_json(&node)?,
        OutputFormat::Yaml => print_yaml(&node)?,
        OutputFormat::Name => {
            let hostname = node
                .description
                .as_ref()
                .and_then(|d| d.hostname.clone())
                .or_else(|| node.spec.as_ref().and_then(|s| s.name.clone()))
                .unwrap_or_default();
            println!("node/{}", hostname);
        }
    }

    Ok(())
}

async fn get_service(
    client: &DockerClient,
    name: &str,
    output: OutputFormat,
) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let service = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|spec| spec.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Service {} not found", name))?;

    match output {
        OutputFormat::Table | OutputFormat::Wide => {
            let spec = service.spec.unwrap_or_default();
            println!("Name: {}", spec.name.unwrap_or_default());
            println!("ID: {}", service.id.unwrap_or_default());
            if let Some(labels) = &spec.labels
                && !labels.is_empty()
            {
                println!("Labels: {}", format_labels(&Some(labels.clone())));
            }
            if let Some(mode) = &spec.mode
                && let Some(replicated) = &mode.replicated
            {
                println!("Replicas: {}", replicated.replicas.unwrap_or(0));
            }
            if let Some(task_template) = &spec.task_template
                && let Some(container_spec) = &task_template.container_spec
            {
                println!(
                    "Image: {}",
                    container_spec.image.as_ref().unwrap_or(&"".to_string())
                );
            }
        }
        OutputFormat::Json => print_json(&service)?,
        OutputFormat::Yaml => print_yaml(&service)?,
        OutputFormat::Name => {
            let spec = service.spec.unwrap_or_default();
            println!("service/{}", spec.name.unwrap_or_default());
        }
    }

    Ok(())
}

async fn get_tasks(
    client: &DockerClient,
    output: OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    let ListOptions {
        show_labels,
        selector,
        field_selector,
        sort_by,
        ..
    } = opts;
    let show_labels = *show_labels;
    let tasks = crate::api::task::list_tasks(client.inner()).await?;

    // Resolve node IDs to hostnames so the NODE column lines up with `get nodes`.
    let node_hostnames: std::collections::HashMap<String, String> =
        match crate::api::node::list_nodes(client.inner()).await {
            Ok(nodes) => nodes
                .into_iter()
                .filter_map(|n| {
                    let id = n.id?;
                    let hostname = n
                        .description
                        .as_ref()
                        .and_then(|d| d.hostname.clone())
                        .or_else(|| n.spec.as_ref().and_then(|s| s.name.clone()))
                        .unwrap_or_else(|| id.clone());
                    Some((id, hostname))
                })
                .collect(),
            // Non-fatal: fall back to showing raw node IDs.
            Err(e) => {
                log::warn!("Could not resolve node hostnames: {}", e);
                Default::default()
            }
        };

    let tasks = keep(
        tasks,
        |t: &bollard::models::Task| {
            t.spec
                .as_ref()
                .and_then(|s| s.container_spec.as_ref())
                .and_then(|c| c.labels.clone())
        },
        selector.as_ref(),
        field_selector.as_ref(),
    );
    let rows: Vec<crate::models::task::TaskRow> =
        crate::cli::rows::task_rows_with_nodes(&tasks, &node_hostnames);

    let mut rows = rows;
    if let Some(sort_field) = sort_by.as_deref() {
        sort_rows(&mut rows, sort_field)?;
    }
    render_rows(&rows, &output, show_labels)?;

    Ok(())
}

async fn get_task(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;
    let task = tasks
        .into_iter()
        .find(|t| {
            t.id.as_ref() == Some(&name.to_string()) || t.name.as_ref() == Some(&name.to_string())
        })
        .ok_or_else(|| anyhow::anyhow!("Task {} not found", name))?;

    match output {
        OutputFormat::Table | OutputFormat::Wide => {
            println!("ID: {}", task.id.unwrap_or_default());
            println!("Name: {}", task.name.unwrap_or_default());
            let labels = task
                .spec
                .as_ref()
                .and_then(|s| s.container_spec.as_ref())
                .and_then(|c| c.labels.clone());
            if let Some(ref lbls) = labels
                && !lbls.is_empty()
            {
                println!("Labels: {}", format_labels(&Some(lbls.clone())));
            }
            if let Some(status) = &task.status {
                println!("Status: {}", status.state.unwrap_or_default());
                if let Some(message) = &status.message {
                    println!("Message: {}", message);
                }
            }
            println!("Desired State: {}", task.desired_state.unwrap_or_default());
            println!("Node ID: {}", task.node_id.unwrap_or_default());
            if let Some(spec) = &task.spec
                && let Some(container_spec) = &spec.container_spec
            {
                println!(
                    "Image: {}",
                    container_spec
                        .image
                        .as_ref()
                        .unwrap_or(&"unknown".to_string())
                );
            }
        }
        OutputFormat::Json => print_json(&task)?,
        OutputFormat::Yaml => print_yaml(&task)?,
        OutputFormat::Name => {
            println!("task/{}", task.id.unwrap_or_default());
        }
    }

    Ok(())
}

async fn get_networks(
    client: &DockerClient,
    output: OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    let ListOptions {
        show_labels,
        selector,
        field_selector,
        sort_by,
        ..
    } = opts;
    let show_labels = *show_labels;
    let networks = crate::api::network::list_networks(client.inner()).await?;

    let networks = keep(
        networks,
        |n: &bollard::models::Network| n.labels.clone(),
        selector.as_ref(),
        field_selector.as_ref(),
    );
    let rows = crate::cli::rows::network_rows(&networks);

    let mut rows = rows;
    if let Some(sort_field) = sort_by.as_deref() {
        sort_rows(&mut rows, sort_field)?;
    }
    render_rows(&rows, &output, show_labels)?;

    Ok(())
}

async fn get_network(
    client: &DockerClient,
    name: &str,
    output: OutputFormat,
) -> anyhow::Result<()> {
    let network = crate::api::network::find_network(client.inner(), name).await?;

    match output {
        OutputFormat::Table | OutputFormat::Wide => {
            println!("Name: {}", network.name.unwrap_or("unknown".to_string()));
            println!("ID: {}", network.id.unwrap_or_default());
            println!(
                "Driver: {}",
                network.driver.unwrap_or_else(|| "unknown".to_string())
            );
            println!(
                "Scope: {}",
                network.scope.unwrap_or_else(|| "unknown".to_string())
            );
            println!("Internal: {}", network.internal.unwrap_or(false));
            if let Some(labels) = &network.labels
                && !labels.is_empty()
            {
                println!("Labels: {}", format_labels(&Some(labels.clone())));
            }
        }
        OutputFormat::Json => print_json(&network)?,
        OutputFormat::Yaml => print_yaml(&network)?,
        OutputFormat::Name => {
            println!("network/{}", network.name.unwrap_or("unknown".to_string()));
        }
    }

    Ok(())
}

async fn get_secrets(
    client: &DockerClient,
    output: OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    let ListOptions {
        show_labels,
        selector,
        field_selector,
        sort_by,
        ..
    } = opts;
    let show_labels = *show_labels;
    let secrets = crate::api::secret::list_secrets(client.inner()).await?;

    let secrets = keep(
        secrets,
        |s: &bollard::models::Secret| s.spec.as_ref().and_then(|sp| sp.labels.clone()),
        selector.as_ref(),
        field_selector.as_ref(),
    );
    let rows = crate::cli::rows::secret_rows(&secrets);

    let mut rows = rows;
    if let Some(sort_field) = sort_by.as_deref() {
        sort_rows(&mut rows, sort_field)?;
    }
    render_rows(&rows, &output, show_labels)?;

    Ok(())
}

async fn get_secret(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let secrets = crate::api::secret::list_secrets(client.inner()).await?;
    let secret = secrets
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|spec| spec.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Secret {} not found", name))?;

    match output {
        OutputFormat::Table | OutputFormat::Wide => {
            println!(
                "Name: {}",
                secret
                    .spec
                    .as_ref()
                    .and_then(|s| s.name.as_ref())
                    .unwrap_or(&"unknown".to_string())
            );
            println!("ID: {}", secret.id.unwrap_or_default());
            println!("Created At: {}", secret.created_at.unwrap_or_default());
            if let Some(labels) = secret.spec.as_ref().and_then(|s| s.labels.as_ref())
                && !labels.is_empty()
            {
                println!("Labels: {}", format_labels(&Some(labels.clone())));
            }
        }
        OutputFormat::Json => print_json(&secret)?,
        OutputFormat::Yaml => print_yaml(&secret)?,
        OutputFormat::Name => {
            let default_name = "unknown".to_string();
            let name = secret
                .spec
                .as_ref()
                .and_then(|s| s.name.as_ref())
                .unwrap_or(&default_name);
            println!("secret/{}", name);
        }
    }

    Ok(())
}

async fn get_configs(
    client: &DockerClient,
    output: OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    let ListOptions {
        show_labels,
        selector,
        field_selector,
        sort_by,
        ..
    } = opts;
    let show_labels = *show_labels;
    let configs = crate::api::config::list_configs(client.inner()).await?;

    let configs = keep(
        configs,
        |c: &bollard::models::Config| c.spec.as_ref().and_then(|sp| sp.labels.clone()),
        selector.as_ref(),
        field_selector.as_ref(),
    );
    let rows = crate::cli::rows::config_rows(&configs);

    let mut rows = rows;
    if let Some(sort_field) = sort_by.as_deref() {
        sort_rows(&mut rows, sort_field)?;
    }
    render_rows(&rows, &output, show_labels)?;

    Ok(())
}

async fn get_config(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let configs = crate::api::config::list_configs(client.inner()).await?;
    let config = configs
        .into_iter()
        .find(|c| c.spec.as_ref().and_then(|spec| spec.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Config {} not found", name))?;

    match output {
        OutputFormat::Table | OutputFormat::Wide => {
            println!(
                "Name: {}",
                config
                    .spec
                    .as_ref()
                    .and_then(|s| s.name.as_ref())
                    .unwrap_or(&"unknown".to_string())
            );
            println!("ID: {}", config.id.unwrap_or_default());
            println!("Created At: {}", config.created_at.unwrap_or_default());
            if let Some(labels) = config.spec.as_ref().and_then(|s| s.labels.as_ref())
                && !labels.is_empty()
            {
                println!("Labels: {}", format_labels(&Some(labels.clone())));
            }
        }
        OutputFormat::Json => print_json(&config)?,
        OutputFormat::Yaml => print_yaml(&config)?,
        OutputFormat::Name => {
            let default_name = "unknown".to_string();
            let name = config
                .spec
                .as_ref()
                .and_then(|s| s.name.as_ref())
                .unwrap_or(&default_name);
            println!("config/{}", name);
        }
    }

    Ok(())
}

async fn get_stack(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let stacks = crate::api::stack::list_stacks(client.inner()).await?;
    let stack = stacks
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| anyhow::anyhow!("Stack '{}' not found", name))?;

    let row = crate::models::stack::StackRow {
        name: stack.name,
        services: stack.services.to_string(),
        replicas: stack.replicas.to_string(),
    };

    match output {
        OutputFormat::Table => print_table(&[row]),
        OutputFormat::Json => print_json(&row)?,
        OutputFormat::Yaml => print_yaml(&row)?,
        OutputFormat::Wide => print_table(&[row]),
        OutputFormat::Name => println!("stack/{}", row.name),
    }

    Ok(())
}

async fn get_stacks(
    client: &DockerClient,
    output: OutputFormat,
    _show_labels: bool,
    _watch: bool,
) -> anyhow::Result<()> {
    let stacks = crate::api::stack::list_stacks(client.inner()).await?;

    let rows: Vec<crate::models::stack::StackRow> = stacks
        .into_iter()
        .map(|s| crate::models::stack::StackRow {
            name: s.name,
            services: s.services.to_string(),
            replicas: s.replicas.to_string(),
        })
        .collect();

    match output {
        OutputFormat::Table => {
            print_table(&rows);
        }
        OutputFormat::Json => print_json(&rows)?,
        OutputFormat::Yaml => print_yaml(&rows)?,
        OutputFormat::Wide => {
            print_table(&rows);
        }
        OutputFormat::Name => {
            for row in &rows {
                println!("stack/{}", row.name);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::matches_field_selector;
    use serde_json::json;

    #[test]
    fn field_selector_accepts_leading_dot_paths() {
        // Regression: the leading-dot form used to build "//status/state" and
        // silently matched nothing.
        let v = json!({"status": {"state": "ready"}});
        assert!(matches_field_selector(&v, ".status.state=ready"));
        assert!(!matches_field_selector(&v, ".status.state=drain"));
    }

    #[test]
    fn field_selector_matches_strings_numbers_and_bools() {
        let v = json!({"status": {"state": "ready"}, "replicas": 3, "active": true});
        assert!(matches_field_selector(&v, "status.state=ready"));
        assert!(matches_field_selector(&v, "replicas=3"));
        assert!(matches_field_selector(&v, "active=true"));
    }

    #[test]
    fn field_selector_rejects_mismatch() {
        let v = json!({"status": {"state": "ready"}});
        assert!(!matches_field_selector(&v, "status.state=drain"));
        assert!(!matches_field_selector(&v, "missing.key=x"));
    }

    #[test]
    fn bare_field_selector_matches_everything() {
        let v = json!({"a": 1});
        assert!(matches_field_selector(&v, "a"));
        assert!(matches_field_selector(&v, "nonsense"));
    }

    #[test]
    fn field_selector_supports_nested_dotted_paths() {
        let v = json!({"spec": {"task": {"template": {"container": {"image": "nginx"}}}}});
        assert!(matches_field_selector(
            &v,
            "spec.task.template.container.image=nginx"
        ));
    }

    #[test]
    fn field_selector_rejects_wrong_type() {
        let v = json!({"replicas": 3});
        assert!(!matches_field_selector(&v, "replicas=notanumber"));
    }
}
