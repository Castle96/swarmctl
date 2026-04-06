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

// Placeholder implementations
async fn describe_task(_client: &DockerClient, _name: &str, _output: OutputFormat) -> anyhow::Result<()> {
    println!("Task description not yet implemented");
    Ok(())
}

async fn describe_network(_client: &DockerClient, _name: &str, _output: OutputFormat) -> anyhow::Result<()> {
    println!("Network description not yet implemented");
    Ok(())
}

async fn describe_secret(_client: &DockerClient, _name: &str, _output: OutputFormat) -> anyhow::Result<()> {
    println!("Secret description not yet implemented");
    Ok(())
}

async fn describe_config(_client: &DockerClient, _name: &str, _output: OutputFormat) -> anyhow::Result<()> {
    println!("Config description not yet implemented");
    Ok(())
}

async fn describe_stack(_client: &DockerClient, _name: &str, _output: OutputFormat) -> anyhow::Result<()> {
    println!("Stack description not yet implemented");
    Ok(())
}