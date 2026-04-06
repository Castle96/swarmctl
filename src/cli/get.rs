use crate::api::client::DockerClient;
use crate::cli::root::{OutputFormat, ResourceType};
use crate::utils::printer::{print_table, print_json, print_yaml};
use crate::models::{node::NodeRow, service::ServiceRow, task::TaskRow, network::NetworkRow};

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: Option<String>,
    output: OutputFormat,
    show_labels: bool,
    selector: Option<String>,
    watch: bool,
) -> anyhow::Result<()> {
    match resource {
        ResourceType::Nodes => {
            if let Some(name) = name {
                // Get specific node
                get_node(client, &name, output).await?;
            } else {
                // List all nodes
                get_nodes(client, output, show_labels, selector, watch).await?;
            }
        }
        ResourceType::Services => {
            if let Some(name) = name {
                // Get specific service
                get_service(client, &name, output).await?;
            } else {
                // List all services
                get_services(client, output, show_labels, selector, watch).await?;
            }
        }
        ResourceType::Tasks => {
            if let Some(name) = name {
                // Get specific task
                get_task(client, &name, output).await?;
            } else {
                // List all tasks
                get_tasks(client, output, show_labels, selector, watch).await?;
            }
        }
        ResourceType::Networks => {
            if let Some(name) = name {
                // Get specific network
                get_network(client, &name, output).await?;
            } else {
                // List all networks
                get_networks(client, output, show_labels, selector, watch).await?;
            }
        }
        ResourceType::Secrets => {
            if let Some(name) = name {
                // Get specific secret
                get_secret(client, &name, output).await?;
            } else {
                // List all secrets
                get_secrets(client, output, show_labels, selector, watch).await?;
            }
        }
        ResourceType::Configs => {
            if let Some(name) = name {
                // Get specific config
                get_config(client, &name, output).await?;
            } else {
                // List all configs
                get_configs(client, output, show_labels, selector, watch).await?;
            }
        }
        ResourceType::Stacks => {
            if let Some(name) = name {
                // Get specific stack
                get_stack(client, &name, output).await?;
            } else {
                // List all stacks
                get_stacks(client, output, show_labels, selector, watch).await?;
            }
        }
    }

    Ok(())
}

async fn get_nodes(
    client: &DockerClient,
    output: OutputFormat,
    show_labels: bool,
    selector: Option<String>,
    watch: bool,
) -> anyhow::Result<()> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;

    let rows: Vec<NodeRow> = nodes
        .into_iter()
        .map(|n| {
            let spec = n.spec.unwrap_or_default();
            let status = n.status.unwrap_or_default();

            let manager = n.manager_status.as_ref().map(|m| {
                match m.reachability.unwrap_or(bollard::models::Reachability::UNKNOWN) {
                    bollard::models::Reachability::REACHABLE => "Reachable",
                    bollard::models::Reachability::UNREACHABLE => "Unavailable",
                    _ => "-",
                }
            }).unwrap_or("-");

            NodeRow {
                id: n.id.unwrap_or_default(),
                hostname: spec.name.unwrap_or_default(),
                status: status.state.unwrap_or(bollard::models::NodeState::READY).to_string(),
                availability: spec.availability
                    .unwrap_or(bollard::models::NodeSpecAvailabilityEnum::ACTIVE)
                    .to_string(),
                manager: manager.to_string(),
            }
        })
        .collect();

    let result = match output {
        OutputFormat::Table => {
            print_table(rows);
            Ok(())
        }
        OutputFormat::Json => print_json(&rows),
        OutputFormat::Yaml => print_yaml(&rows),
    };

    result
}

async fn get_services(
    client: &DockerClient,
    output: OutputFormat,
    show_labels: bool,
    selector: Option<String>,
    watch: bool,
) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;

    let rows: Vec<ServiceRow> = services
        .into_iter()
        .map(|s| {
            let spec = s.spec.unwrap_or_default();

            let name = spec.name.unwrap_or_default();
            let image = spec
                .task_template
                .and_then(|t| t.container_spec)
                .and_then(|c| c.image)
                .unwrap_or_default();

            let (mode, replicas) = match spec.mode {
                Some(m) if m.replicated.is_some() => {
                    let r = m.replicated.unwrap().replicas.unwrap_or(0);
                    ("replicated".to_string(), format!("{}/{}", r, r))
                }
                Some(_) => ("global".to_string(), "N/A".to_string()),
                None => ("unknown".to_string(), "N/A".to_string()),
            };

            ServiceRow {
                id: s.id.unwrap_or_default(),
                name,
                mode,
                replicas,
                image,
            }
        })
        .collect();

    let result = match output {
        OutputFormat::Table => {
            print_table(rows);
            Ok(())
        }
        OutputFormat::Json => print_json(&rows),
        OutputFormat::Yaml => print_yaml(&rows),
    };

    result
}

async fn get_node(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;
    let node = nodes.into_iter()
        .find(|n| n.spec.as_ref().and_then(|s| s.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Node {} not found", name))?;

    match output {
        OutputFormat::Table => {
            let spec = node.spec.unwrap_or_default();
            let status = node.status.unwrap_or_default();
            println!("Name: {}", spec.name.unwrap_or_default());
            println!("ID: {}", node.id.unwrap_or_default());
            println!("Status: {}", status.state.unwrap_or(bollard::models::NodeState::READY));
            println!("Availability: {}", spec.availability.unwrap_or(bollard::models::NodeSpecAvailabilityEnum::ACTIVE));
            if let Some(manager_status) = &node.manager_status {
                println!("Manager Status: {}", manager_status.reachability.unwrap_or(bollard::models::Reachability::UNKNOWN));
            }
        }
        OutputFormat::Json => print_json(&node)?,
        OutputFormat::Yaml => print_yaml(&node)?,
    }

    Ok(())
}

async fn get_service(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let service = services.into_iter()
        .find(|s| s.spec.as_ref().and_then(|spec| spec.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Service {} not found", name))?;

    match output {
        OutputFormat::Table => {
            let spec = service.spec.unwrap_or_default();
            println!("Name: {}", spec.name.unwrap_or_default());
            println!("ID: {}", service.id.unwrap_or_default());
            if let Some(mode) = &spec.mode {
                if let Some(replicated) = &mode.replicated {
                    println!("Replicas: {}", replicated.replicas.unwrap_or(0));
                }
            }
            if let Some(task_template) = &spec.task_template {
                if let Some(container_spec) = &task_template.container_spec {
                    println!("Image: {}", container_spec.image.as_ref().unwrap_or(&"".to_string()));
                }
            }
        }
        OutputFormat::Json => print_json(&service)?,
        OutputFormat::Yaml => print_yaml(&service)?,
    }

    Ok(())
}

// Placeholder implementations for other resources
async fn get_tasks(_client: &DockerClient, _output: OutputFormat, _show_labels: bool, _selector: Option<String>, _watch: bool) -> anyhow::Result<()> {
    println!("Task listing not yet implemented");
    Ok(())
}

async fn get_task(_client: &DockerClient, _name: &str, _output: OutputFormat) -> anyhow::Result<()> {
    println!("Task inspection not yet implemented");
    Ok(())
}

async fn get_networks(_client: &DockerClient, _output: OutputFormat, _show_labels: bool, _selector: Option<String>, _watch: bool) -> anyhow::Result<()> {
    println!("Network listing not yet implemented");
    Ok(())
}

async fn get_network(_client: &DockerClient, _name: &str, _output: OutputFormat) -> anyhow::Result<()> {
    println!("Network inspection not yet implemented");
    Ok(())
}

async fn get_secrets(_client: &DockerClient, _output: OutputFormat, _show_labels: bool, _selector: Option<String>, _watch: bool) -> anyhow::Result<()> {
    println!("Secret listing not yet implemented");
    Ok(())
}

async fn get_secret(_client: &DockerClient, _name: &str, _output: OutputFormat) -> anyhow::Result<()> {
    println!("Secret inspection not yet implemented");
    Ok(())
}

async fn get_configs(_client: &DockerClient, _output: OutputFormat, _show_labels: bool, _selector: Option<String>, _watch: bool) -> anyhow::Result<()> {
    println!("Config listing not yet implemented");
    Ok(())
}

async fn get_config(_client: &DockerClient, _name: &str, _output: OutputFormat) -> anyhow::Result<()> {
    println!("Config inspection not yet implemented");
    Ok(())
}

async fn get_stacks(_client: &DockerClient, _output: OutputFormat, _show_labels: bool, _selector: Option<String>, _watch: bool) -> anyhow::Result<()> {
    println!("Stack listing not yet implemented");
    Ok(())
}

async fn get_stack(_client: &DockerClient, _name: &str, _output: OutputFormat) -> anyhow::Result<()> {
    println!("Stack inspection not yet implemented");
    Ok(())
}