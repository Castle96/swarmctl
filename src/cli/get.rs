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
    _show_labels: bool,
    _selector: Option<String>,
    _watch: bool,
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
    _show_labels: bool,
    _selector: Option<String>,
    _watch: bool,
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

async fn get_tasks(client: &DockerClient, output: OutputFormat, _show_labels: bool, _selector: Option<String>, _watch: bool) -> anyhow::Result<()> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;

    let rows: Vec<crate::models::task::TaskRow> = tasks
        .into_iter()
        .map(|t| {
            crate::models::task::TaskRow {
                id: t.id.unwrap_or_default(),
                name: t.name.unwrap_or_default(),
                desired_state: format!("{:?}", t.desired_state.unwrap_or(bollard::models::TaskState::RUNNING)),
                current_state: t.status.as_ref().and_then(|s| s.state.clone()).map(|v| format!("{:?}", v)).unwrap_or_default(),
                image: t.spec.as_ref().and_then(|s| s.container_spec.as_ref()).and_then(|c| c.image.clone()).unwrap_or_default(),
                ports: "".to_string(),
                node: t.node_id.unwrap_or_default(),
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

async fn get_task(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;
    let task = tasks.into_iter()
        .find(|t| t.id.as_ref() == Some(&name.to_string()) || t.name.as_ref() == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Task {} not found", name))?;

    match output {
        OutputFormat::Table => {
            println!("ID: {}", task.id.unwrap_or_default());
            println!("Name: {}", task.name.unwrap_or_default());
            if let Some(status) = &task.status {
                println!("Status: {}", status.state.clone().unwrap_or_default());
                if let Some(message) = &status.message {
                    println!("Message: {}", message);
                }
            }
            println!("Desired State: {}", task.desired_state.unwrap_or_default());
            println!("Node ID: {}", task.node_id.unwrap_or_default());
            if let Some(spec) = &task.spec {
                if let Some(container_spec) = &spec.container_spec {
                    println!("Image: {}", container_spec.image.as_ref().unwrap_or(&"unknown".to_string()));
                }
            }
        }
        OutputFormat::Json => print_json(&task)?,
        OutputFormat::Yaml => print_yaml(&task)?,
    }

    Ok(())
}

async fn get_networks(client: &DockerClient, output: OutputFormat, _show_labels: bool, _selector: Option<String>, _watch: bool) -> anyhow::Result<()> {
    let networks = crate::api::network::list_networks(client.inner()).await?;

    let rows: Vec<NetworkRow> = networks
        .into_iter()
        .map(|n| {
            let scope = n.scope.unwrap_or_else(|| "unknown".to_string());
            let internal = n.internal.unwrap_or(false);
            NetworkRow {
                id: n.id.unwrap_or_default(),
                name: n.name.unwrap_or("unknown".to_string()),
                driver: n.driver.unwrap_or_else(|| "unknown".to_string()),
                scope,
                internal: if internal { "true" } else { "false" }.to_string(),
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

async fn get_network(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let networks = crate::api::network::list_networks(client.inner()).await?;
    let network = networks.into_iter()
        .find(|n| n.name.as_ref() == Some(&name.to_string()) || n.id.as_ref() == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Network {} not found", name))?;

    match output {
        OutputFormat::Table => {
            println!("Name: {}", network.name.unwrap_or("unknown".to_string()));
            println!("ID: {}", network.id.unwrap_or_default());
            println!("Driver: {}", network.driver.unwrap_or_else(|| "unknown".to_string()));
            println!("Scope: {}", network.scope.unwrap_or_else(|| "unknown".to_string()));
            println!("Internal: {}", network.internal.unwrap_or(false));
        }
        OutputFormat::Json => print_json(&network)?,
        OutputFormat::Yaml => print_yaml(&network)?,
    }

    Ok(())
}

async fn get_secrets(client: &DockerClient, output: OutputFormat, _show_labels: bool, _selector: Option<String>, _watch: bool) -> anyhow::Result<()> {
    let secrets = crate::api::secret::list_secrets(client.inner()).await?;

    let rows: Vec<crate::models::secret::SecretRow> = secrets
        .into_iter()
        .map(|s| {
            crate::models::secret::SecretRow {
                id: s.id.unwrap_or_default(),
                name: s.spec.unwrap_or_default().name.unwrap_or_default(),
                created_at: s.created_at.unwrap_or_default(),
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

async fn get_secret(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let secrets = crate::api::secret::list_secrets(client.inner()).await?;
    let secret = secrets.into_iter()
        .find(|s| s.spec.as_ref().and_then(|spec| spec.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Secret {} not found", name))?;

    match output {
        OutputFormat::Table => {
            println!("Name: {}", secret.spec.as_ref().and_then(|s| s.name.as_ref()).unwrap_or(&"unknown".to_string()));
            println!("ID: {}", secret.id.unwrap_or_default());
            println!("Created At: {}", secret.created_at.unwrap_or_default());
        }
        OutputFormat::Json => print_json(&secret)?,
        OutputFormat::Yaml => print_yaml(&secret)?,
    }

    Ok(())
}

async fn get_configs(client: &DockerClient, output: OutputFormat, _show_labels: bool, _selector: Option<String>, _watch: bool) -> anyhow::Result<()> {
    let configs = crate::api::config::list_configs(client.inner()).await?;

    let rows: Vec<crate::models::config::ConfigRow> = configs
        .into_iter()
        .map(|c| {
            crate::models::config::ConfigRow {
                id: c.id.unwrap_or_default(),
                name: c.spec.unwrap_or_default().name.unwrap_or_default(),
                created_at: c.created_at.unwrap_or_default(),
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

async fn get_config(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let configs = crate::api::config::list_configs(client.inner()).await?;
    let config = configs.into_iter()
        .find(|c| c.spec.as_ref().and_then(|spec| spec.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Config {} not found", name))?;

    match output {
        OutputFormat::Table => {
            println!("Name: {}", config.spec.as_ref().and_then(|s| s.name.as_ref()).unwrap_or(&"unknown".to_string()));
            println!("ID: {}", config.id.unwrap_or_default());
            println!("Created At: {}", config.created_at.unwrap_or_default());
        }
        OutputFormat::Json => print_json(&config)?,
        OutputFormat::Yaml => print_yaml(&config)?,
    }

    Ok(())
}

async fn get_stacks(client: &DockerClient, output: OutputFormat, _show_labels: bool, _selector: Option<String>, _watch: bool) -> anyhow::Result<()> {
    let stacks = crate::api::stack::list_stacks(client.inner()).await?;

    let rows: Vec<crate::models::stack::StackRow> = stacks
        .into_iter()
        .map(|s| {
            crate::models::stack::StackRow {
                name: s.name,
                services: s.services.to_string(),
                replicas: s.replicas.to_string(),
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

async fn get_stack(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let services = crate::api::stack::get_stack_services(client.inner(), name).await?;

    if services.is_empty() {
        return Err(anyhow::anyhow!("Stack {} not found", name));
    }

    match output {
        OutputFormat::Table => {
            println!("Stack: {}", name);
            println!("Services: {}", services.len());
            println!();
            println!("{:<40} {:<15} {:<15}", "SERVICE", "IMAGE", "REPLICAS");
            println!("{}", "-".repeat(70));
            
            for service in &services {
                let spec = service.spec.as_ref();
                let name = spec.and_then(|s| s.name.clone()).unwrap_or_default();
                let image = spec.and_then(|s| s.task_template.as_ref())
                    .and_then(|t| t.container_spec.as_ref())
                    .and_then(|c| c.image.clone())
                    .unwrap_or_default();
                let replicas = spec.and_then(|s| s.mode.as_ref())
                    .and_then(|m| m.replicated.as_ref())
                    .and_then(|r| r.replicas)
                    .map(|r| r.to_string())
                    .unwrap_or_else(|| "global".to_string());
                
                println!("{:<40} {:<15} {:<15}", name, image.split(':').next().unwrap_or(&image), replicas);
            }
        }
        OutputFormat::Json => print_json(&services)?,
        OutputFormat::Yaml => print_yaml(&services)?,
    }

    Ok(())
}