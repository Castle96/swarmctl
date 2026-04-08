use crate::api::client::DockerClient;
use crate::cli::root::{OutputFormat, ResourceType};

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: String,
    output: OutputFormat,
) -> anyhow::Result<()> {
    match resource {
        ResourceType::Nodes => describe_node(client, &name, output).await?,
        ResourceType::Services => describe_service(client, &name, output).await?,
        ResourceType::Tasks => describe_task(client, &name, output).await?,
        ResourceType::Networks => describe_network(client, &name, output).await?,
        ResourceType::Secrets => describe_secret(client, &name, output).await?,
        ResourceType::Configs => describe_config(client, &name, output).await?,
        ResourceType::Stacks => describe_stack(client, &name, output).await?,
    }

    Ok(())
}

async fn describe_node(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;
    let node = nodes.into_iter()
        .find(|n| n.id.as_ref() == Some(&name.to_string()) || n.spec.as_ref().and_then(|s| s.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Node {} not found", name))?;

    match output {
        OutputFormat::Table => {
            let spec = node.spec.as_ref().unwrap();
            let status = node.status.as_ref().unwrap();

            println!("Name:\t{}", spec.name.as_ref().unwrap_or(&"".to_string()));
            println!("ID:\t{}", node.id.as_ref().unwrap_or(&"".to_string()));
            println!("Status:\t{}", status.state.as_ref().unwrap_or(&bollard::models::NodeState::READY));
            println!("Availability:\t{}", spec.availability.as_ref().unwrap_or(&bollard::models::NodeSpecAvailabilityEnum::ACTIVE));

            if let Some(manager_status) = &node.manager_status {
                println!("Manager Status:\t{}", manager_status.reachability.as_ref().unwrap_or(&bollard::models::Reachability::UNKNOWN));
                println!("Manager Address:\t{}", manager_status.addr.as_ref().unwrap_or(&"".to_string()));
            }

            if let Some(description) = &node.description {
                if let Some(hostname) = &description.hostname {
                    println!("Hostname:\t{}", hostname);
                }
                if let Some(platform) = &description.platform {
                    println!("Platform:\t{}", platform.architecture.as_ref().unwrap_or(&"".to_string()));
                    println!("OS:\t{}", platform.os.as_ref().unwrap_or(&"".to_string()));
                }
            }

            println!("Created:\t{}", node.created_at.as_ref().unwrap_or(&"".to_string()));
            println!("Updated:\t{}", node.updated_at.as_ref().unwrap_or(&"".to_string()));
        }
        OutputFormat::Json => crate::utils::printer::print_json(&node)?,
        OutputFormat::Yaml => crate::utils::printer::print_yaml(&node)?,
    }

    Ok(())
}

async fn describe_service(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let service = services.into_iter()
        .find(|s| s.spec.as_ref().and_then(|spec| spec.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Service {} not found", name))?;

    match output {
        OutputFormat::Table => {
            let spec = service.spec.as_ref().unwrap();

            println!("Name:\t{}", spec.name.as_ref().unwrap_or(&"".to_string()));
            println!("ID:\t{}", service.id.as_ref().unwrap_or(&"".to_string()));

            if let Some(mode) = &spec.mode {
                if let Some(replicated) = &mode.replicated {
                    println!("Mode:\tReplicated");
                    println!("Replicas:\t{}", replicated.replicas.unwrap_or(0));
                } else if mode.global.is_some() {
                    println!("Mode:\tGlobal");
                }
            }

            if let Some(task_template) = &spec.task_template {
                if let Some(container_spec) = &task_template.container_spec {
                    println!("Image:\t{}", container_spec.image.as_ref().unwrap_or(&"".to_string()));

                    if let Some(args) = &container_spec.args {
                        println!("Args:\t{}", args.join(" "));
                    }

                    if let Some(env) = &container_spec.env {
                        println!("Environment:");
                        for env_var in env {
                            println!("\t{}", env_var);
                        }
                    }
                }

                if let Some(resources) = &task_template.resources {
                    if let Some(limits) = &resources.limits {
                        println!("CPU Limit:\t{} cores", limits.nano_cpus.unwrap_or(0) as f64 / 1_000_000_000.0);
                        println!("Memory Limit:\t{} bytes", limits.memory_bytes.unwrap_or(0));
                    }
                }
            }

            if let Some(endpoint) = &spec.endpoint_spec {
                if let Some(ports) = &endpoint.ports {
                    println!("Ports:");
                    for port in ports {
                        println!("\t{}:{} -> {}", port.published_port.unwrap_or(0), port.target_port.unwrap_or(0), port.protocol.as_ref().unwrap_or(&bollard::models::EndpointPortConfigProtocolEnum::TCP).as_ref());
                    }
                }
            }

            println!("Created:\t{}", service.created_at.as_ref().unwrap_or(&"".to_string()));
            println!("Updated:\t{}", service.updated_at.as_ref().unwrap_or(&"".to_string()));
        }
        OutputFormat::Json => crate::utils::printer::print_json(&service)?,
        OutputFormat::Yaml => crate::utils::printer::print_yaml(&service)?,
    }

    Ok(())
}

async fn describe_task(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;
    let task = tasks.into_iter()
        .find(|t| t.id.as_ref() == Some(&name.to_string()) || t.name.as_ref() == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Task {} not found", name))?;

    match output {
        OutputFormat::Table => {
            println!("ID:\t{}", task.id.unwrap_or_default());
            println!("Name:\t{}", task.name.unwrap_or_default());
            
            if let Some(status) = &task.status {
                println!("Status:\t{}", status.state.clone().unwrap_or_default());
                if let Some(message) = &status.message {
                    println!("Message:\t{}", message);
                }
            }
            
            println!("Desired State:\t{}", task.desired_state.unwrap_or_default());
            println!("Node ID:\t{}", task.node_id.unwrap_or_default());
            
            if let Some(spec) = &task.spec {
                if let Some(container_spec) = &spec.container_spec {
                    println!("Image:\t{}", container_spec.image.as_ref().unwrap_or(&"unknown".to_string()));
                }
                
                if let Some(resources) = &spec.resources {
                    if let Some(limits) = &resources.limits {
                        println!("CPU Limit:\t{} cores", limits.nano_cpus.unwrap_or(0) as f64 / 1_000_000_000.0);
                        println!("Memory Limit:\t{} bytes", limits.memory_bytes.unwrap_or(0));
                    }
                }
            }
            
            println!("Created At:\t{}", task.created_at.unwrap_or_default());
            println!("Updated At:\t{}", task.updated_at.unwrap_or_default());
        }
        OutputFormat::Json => crate::utils::printer::print_json(&task)?,
        OutputFormat::Yaml => crate::utils::printer::print_yaml(&task)?,
    }

    Ok(())
}

async fn describe_network(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let networks = crate::api::network::list_networks(client.inner()).await?;
    let network = networks.into_iter()
        .find(|n| n.name.as_ref() == Some(&name.to_string()) || n.id.as_ref() == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Network {} not found", name))?;

    match output {
        OutputFormat::Table => {
            println!("Name:\t{}", network.name.unwrap_or_default());
            println!("ID:\t{}", network.id.unwrap_or_default());
            println!("Driver:\t{}", network.driver.unwrap_or_else(|| "unknown".to_string()));
            println!("Scope:\t{}", network.scope.unwrap_or_else(|| "unknown".to_string()));
            println!("Internal:\t{}", network.internal.unwrap_or(false));
        }
        OutputFormat::Json => crate::utils::printer::print_json(&network)?,
        OutputFormat::Yaml => crate::utils::printer::print_yaml(&network)?,
    }

    Ok(())
}

async fn describe_secret(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let secrets = crate::api::secret::list_secrets(client.inner()).await?;
    let secret = secrets.into_iter()
        .find(|s| s.spec.as_ref().and_then(|spec| spec.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Secret {} not found", name))?;

    match output {
        OutputFormat::Table => {
            println!("Name:\t{}", secret.spec.as_ref().and_then(|s| s.name.as_ref()).unwrap_or(&"unknown".to_string()));
            println!("ID:\t{}", secret.id.unwrap_or_default());
            println!("Created At:\t{}", secret.created_at.unwrap_or_default());
            
            if let Some(spec) = &secret.spec {
                if let Some(labels) = &spec.labels {
                    println!("Labels:");
                    for (key, value) in labels {
                        println!("  {}:\t{}", key, value);
                    }
                }
            }
        }
        OutputFormat::Json => crate::utils::printer::print_json(&secret)?,
        OutputFormat::Yaml => crate::utils::printer::print_yaml(&secret)?,
    }

    Ok(())
}

async fn describe_config(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let configs = crate::api::config::list_configs(client.inner()).await?;
    let config = configs.into_iter()
        .find(|c| c.spec.as_ref().and_then(|spec| spec.name.as_ref()) == Some(&name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("Config {} not found", name))?;

    match output {
        OutputFormat::Table => {
            println!("Name:\t{}", config.spec.as_ref().and_then(|s| s.name.as_ref()).unwrap_or(&"unknown".to_string()));
            println!("ID:\t{}", config.id.unwrap_or_default());
            println!("Created At:\t{}", config.created_at.unwrap_or_default());
            
            if let Some(spec) = &config.spec {
                if let Some(labels) = &spec.labels {
                    println!("Labels:");
                    for (key, value) in labels {
                        println!("  {}:\t{}", key, value);
                    }
                }
            }
        }
        OutputFormat::Json => crate::utils::printer::print_json(&config)?,
        OutputFormat::Yaml => crate::utils::printer::print_yaml(&config)?,
    }

    Ok(())
}

async fn describe_stack(client: &DockerClient, name: &str, output: OutputFormat) -> anyhow::Result<()> {
    let services = crate::api::stack::get_stack_services(client.inner(), name).await?;

    if services.is_empty() {
        return Err(anyhow::anyhow!("Stack {} not found", name));
    }

    match output {
        OutputFormat::Table => {
            println!("Stack:\t{}", name);
            println!("Services:\t{}", services.len());
            println!();
            println!("{:<40} {:<15} {:<15}", "SERVICE", "IMAGE", "REPLICAS");
            println!("{}", "─".repeat(70));
            
            for service in &services {
                let spec = service.spec.as_ref();
                let svc_name = spec.and_then(|s| s.name.clone()).unwrap_or_default();
                let image = spec.and_then(|s| s.task_template.as_ref())
                    .and_then(|t| t.container_spec.as_ref())
                    .and_then(|c| c.image.clone())
                    .unwrap_or_default();
                let replicas = spec.and_then(|s| s.mode.as_ref())
                    .and_then(|m| m.replicated.as_ref())
                    .and_then(|r| r.replicas)
                    .map(|r| r.to_string())
                    .unwrap_or_else(|| "global".to_string());
                
                println!("{:<40} {:<15} {:<15}", svc_name, image.split(':').next().unwrap_or(&image), replicas);
            }
        }
        OutputFormat::Json => crate::utils::printer::print_json(&services)?,
        OutputFormat::Yaml => crate::utils::printer::print_yaml(&services)?,
    }

    Ok(())
}