use crate::api::client::DockerClient;
use crate::cli::root::{OutputFormat, ResourceType};
use crate::models::{network::NetworkRow, node::NodeRow, service::ServiceRow};
use crate::utils::printer::{print_json, print_table, print_yaml};
use crate::utils::selectors::{format_labels, matches_selector};

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: Option<String>,
    output: OutputFormat,
    show_labels: bool,
    selector: Option<String>,
    field_selector: Option<String>,
    sort_by: Option<String>,
    all_namespaces: bool,
    watch: bool,
) -> anyhow::Result<()> {
    loop {
        match resource {
            ResourceType::Nodes => {
                if let Some(name) = &name {
                    get_node(client, name, output.clone()).await?;
                } else {
                    get_nodes(
                        client,
                        output.clone(),
                        show_labels,
                        selector.clone(),
                        field_selector.clone(),
                        sort_by.clone(),
                        watch,
                    )
                    .await?;
                }
            }
            ResourceType::Services => {
                if let Some(name) = &name {
                    get_service(client, name, output.clone()).await?;
                } else {
                    get_services(
                        client,
                        output.clone(),
                        show_labels,
                        selector.clone(),
                        field_selector.clone(),
                        sort_by.clone(),
                        watch,
                    )
                    .await?;
                }
            }
            ResourceType::Tasks => {
                if let Some(name) = &name {
                    get_task(client, name, output.clone()).await?;
                } else {
                    get_tasks(
                        client,
                        output.clone(),
                        show_labels,
                        selector.clone(),
                        field_selector.clone(),
                        sort_by.clone(),
                        watch,
                    )
                    .await?;
                }
            }
            ResourceType::Networks => {
                if let Some(name) = &name {
                    get_network(client, name, output.clone()).await?;
                } else {
                    get_networks(
                        client,
                        output.clone(),
                        show_labels,
                        selector.clone(),
                        field_selector.clone(),
                        sort_by.clone(),
                        watch,
                    )
                    .await?;
                }
            }
            ResourceType::Secrets => {
                if let Some(name) = &name {
                    get_secret(client, name, output.clone()).await?;
                } else {
                    get_secrets(
                        client,
                        output.clone(),
                        show_labels,
                        selector.clone(),
                        field_selector.clone(),
                        sort_by.clone(),
                        watch,
                    )
                    .await?;
                }
            }
            ResourceType::Configs => {
                if let Some(name) = &name {
                    get_config(client, name, output.clone()).await?;
                } else {
                    get_configs(
                        client,
                        output.clone(),
                        show_labels,
                        selector.clone(),
                        field_selector.clone(),
                        sort_by.clone(),
                        watch,
                    )
                    .await?;
                }
            }
            ResourceType::Stacks => {
                if let Some(name) = &name {
                    get_stack(client, name, output.clone()).await?;
                } else {
                    get_stacks(client, output.clone(), show_labels, watch).await?;
                }
            }
        }

        if !watch {
            break;
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    Ok(())
}

fn matches_field_selector(
    value: &serde_json::Value,
    field_selector: &str,
) -> bool {
    let Some((key, expected)) = field_selector.split_once('=') else {
        return true;
    };
    let actual = value.pointer(&format!("/{}", key.replace('.', "/")));
    match actual {
        Some(serde_json::Value::String(s)) => s == expected,
        Some(serde_json::Value::Number(n)) => n.to_string() == expected,
        Some(serde_json::Value::Bool(b)) => b.to_string() == expected,
        _ => false,
    }
}

fn sort_by_path(rows: &mut Vec<serde_json::Value>, path: &str) {
    let json_path = format!("/{}", path.replace('.', "/"));
    rows.sort_by(|a, b| {
        let a_val = a.pointer(&json_path).unwrap_or(&serde_json::Value::Null);
        let b_val = b.pointer(&json_path).unwrap_or(&serde_json::Value::Null);
        a_val.to_string().cmp(&b_val.to_string())
    });
}

async fn get_nodes(
    client: &DockerClient,
    output: OutputFormat,
    show_labels: bool,
    selector: Option<String>,
    field_selector: Option<String>,
    sort_by: Option<String>,
    _watch: bool,
) -> anyhow::Result<()> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;

    let rows: Vec<NodeRow> = nodes
        .into_iter()
        .filter(|n| {
            selector
                .as_ref()
                .map(|sel| matches_selector(&n.spec.as_ref().and_then(|s| s.labels.clone()), sel))
                .unwrap_or(true)
        })
        .filter(|n| {
            field_selector
                .as_ref()
                .map(|sel| {
                    let json = serde_json::to_value(n).unwrap_or_default();
                    matches_field_selector(&json, sel)
                })
                .unwrap_or(true)
        })
        .map(|n| {
            let spec = n.spec.unwrap_or_default();
            let status = n.status.unwrap_or_default();

            let manager = n
                .manager_status
                .as_ref()
                .map(|m| {
                    match m
                        .reachability
                        .unwrap_or(bollard::models::Reachability::UNKNOWN)
                    {
                        bollard::models::Reachability::REACHABLE => "Reachable",
                        bollard::models::Reachability::UNREACHABLE => "Unavailable",
                        _ => "-",
                    }
                })
                .unwrap_or("-");

            NodeRow {
                id: n.id.unwrap_or_default(),
                hostname: spec.name.unwrap_or_default(),
                status: status
                    .state
                    .unwrap_or(bollard::models::NodeState::READY)
                    .to_string(),
                availability: spec
                    .availability
                    .unwrap_or(bollard::models::NodeSpecAvailabilityEnum::ACTIVE)
                    .to_string(),
                manager: manager.to_string(),
                labels: format_labels(&spec.labels),
            }
        })
        .collect();

    match output {
        OutputFormat::Table => {
            print_table(&rows);
            if show_labels {
                for row in &rows {
                    if !row.labels.is_empty() {
                        println!("  Labels: {}", row.labels);
                    }
                }
            }
        }
        OutputFormat::Json => print_json(&rows)?,
        OutputFormat::Yaml => print_yaml(&rows)?,
        OutputFormat::Wide => {
            print_table(&rows);
            if show_labels {
                for row in &rows {
                    if !row.labels.is_empty() {
                        println!("  Labels: {}", row.labels);
                    }
                }
            }
        }
        OutputFormat::Name => {
            for row in &rows {
                println!("node/{}", row.hostname);
            }
        }
    }

    if let Some(sort_field) = sort_by {
        let mut json_rows: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| serde_json::to_value(r).unwrap_or_default())
            .collect();
        sort_by_path(&mut json_rows, &sort_field);
        print_json(&json_rows)?;
    }

    Ok(())
}

async fn get_services(
    client: &DockerClient,
    output: OutputFormat,
    show_labels: bool,
    selector: Option<String>,
    field_selector: Option<String>,
    sort_by: Option<String>,
    _watch: bool,
) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;

    let rows: Vec<ServiceRow> = services
        .into_iter()
        .filter(|s| {
            selector
                .as_ref()
                .map(|sel| {
                    matches_selector(&s.spec.as_ref().and_then(|spec| spec.labels.clone()), sel)
                })
                .unwrap_or(true)
        })
        .filter(|s| {
            field_selector
                .as_ref()
                .map(|sel| {
                    let json = serde_json::to_value(s).unwrap_or_default();
                    matches_field_selector(&json, sel)
                })
                .unwrap_or(true)
        })
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
                labels: format_labels(&spec.labels),
            }
        })
        .collect();

    match output {
        OutputFormat::Table => {
            print_table(&rows);
            if show_labels {
                for row in &rows {
                    if !row.labels.is_empty() {
                        println!("  Labels: {}", row.labels);
                    }
                }
            }
        }
        OutputFormat::Json => print_json(&rows)?,
        OutputFormat::Yaml => print_yaml(&rows)?,
        OutputFormat::Wide => {
            print_table(&rows);
            if show_labels {
                for row in &rows {
                    if !row.labels.is_empty() {
                        println!("  Labels: {}", row.labels);
                    }
                }
            }
        }
        OutputFormat::Name => {
            for row in &rows {
                println!("service/{}", row.name);
            }
        }
    }

    if let Some(sort_field) = sort_by {
        let mut json_rows: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| serde_json::to_value(r).unwrap_or_default())
            .collect();
        sort_by_path(&mut json_rows, &sort_field);
        print_json(&json_rows)?;
    }

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
            let spec = node.spec.unwrap_or_default();
            let status = node.status.unwrap_or_default();
            println!("Name: {}", spec.name.unwrap_or_default());
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
            let spec = node.spec.unwrap_or_default();
            println!("node/{}", spec.name.unwrap_or_default());
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
    show_labels: bool,
    selector: Option<String>,
    field_selector: Option<String>,
    sort_by: Option<String>,
    _watch: bool,
) -> anyhow::Result<()> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;

    let rows: Vec<crate::models::task::TaskRow> = tasks
        .into_iter()
        .filter(|t| {
            selector
                .as_ref()
                .map(|sel| {
                    matches_selector(
                        &t.spec
                            .as_ref()
                            .and_then(|s| s.container_spec.as_ref())
                            .and_then(|c| c.labels.clone()),
                        sel,
                    )
                })
                .unwrap_or(true)
        })
        .filter(|t| {
            field_selector
                .as_ref()
                .map(|sel| {
                    let json = serde_json::to_value(t).unwrap_or_default();
                    matches_field_selector(&json, sel)
                })
                .unwrap_or(true)
        })
        .map(|t| {
            let labels = t
                .spec
                .as_ref()
                .and_then(|s| s.container_spec.as_ref())
                .and_then(|c| c.labels.clone());
            crate::models::task::TaskRow {
                id: t.id.unwrap_or_default(),
                name: t.name.unwrap_or_default(),
                desired_state: format!(
                    "{:?}",
                    t.desired_state
                        .unwrap_or(bollard::models::TaskState::RUNNING)
                ),
                current_state: t
                    .status
                    .as_ref()
                    .and_then(|s| s.state)
                    .map(|v| format!("{:?}", v))
                    .unwrap_or_default(),
                image: t
                    .spec
                    .as_ref()
                    .and_then(|s| s.container_spec.as_ref())
                    .and_then(|c| c.image.clone())
                    .unwrap_or_default(),
                ports: "".to_string(),
                node: t.node_id.unwrap_or_default(),
                labels: format_labels(&labels),
            }
        })
        .collect();

    match output {
        OutputFormat::Table => {
            print_table(&rows);
            if show_labels {
                for row in &rows {
                    if !row.labels.is_empty() {
                        println!("  Labels: {}", row.labels);
                    }
                }
            }
        }
        OutputFormat::Json => print_json(&rows)?,
        OutputFormat::Yaml => print_yaml(&rows)?,
        OutputFormat::Wide => {
            print_table(&rows);
            if show_labels {
                for row in &rows {
                    if !row.labels.is_empty() {
                        println!("  Labels: {}", row.labels);
                    }
                }
            }
        }
        OutputFormat::Name => {
            for row in &rows {
                println!("task/{}", row.id);
            }
        }
    }

    if let Some(sort_field) = sort_by {
        let mut json_rows: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| serde_json::to_value(r).unwrap_or_default())
            .collect();
        sort_by_path(&mut json_rows, &sort_field);
        print_json(&json_rows)?;
    }

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
    show_labels: bool,
    selector: Option<String>,
    field_selector: Option<String>,
    sort_by: Option<String>,
    _watch: bool,
) -> anyhow::Result<()> {
    let networks = crate::api::network::list_networks(client.inner()).await?;

    let rows: Vec<NetworkRow> = networks
        .into_iter()
        .filter(|n| {
            selector
                .as_ref()
                .map(|sel| matches_selector(&n.labels.clone(), sel))
                .unwrap_or(true)
        })
        .filter(|n| {
            field_selector
                .as_ref()
                .map(|sel| {
                    let json = serde_json::to_value(n).unwrap_or_default();
                    matches_field_selector(&json, sel)
                })
                .unwrap_or(true)
        })
        .map(|n| {
            let scope = n.scope.unwrap_or_else(|| "unknown".to_string());
            let internal = n.internal.unwrap_or(false);
            NetworkRow {
                id: n.id.unwrap_or_default(),
                name: n.name.unwrap_or("unknown".to_string()),
                driver: n.driver.unwrap_or_else(|| "unknown".to_string()),
                scope,
                internal: if internal { "true" } else { "false" }.to_string(),
                labels: format_labels(&n.labels),
            }
        })
        .collect();

    match output {
        OutputFormat::Table => {
            print_table(&rows);
            if show_labels {
                for row in &rows {
                    if !row.labels.is_empty() {
                        println!("  Labels: {}", row.labels);
                    }
                }
            }
        }
        OutputFormat::Json => print_json(&rows)?,
        OutputFormat::Yaml => print_yaml(&rows)?,
        OutputFormat::Wide => {
            print_table(&rows);
            if show_labels {
                for row in &rows {
                    if !row.labels.is_empty() {
                        println!("  Labels: {}", row.labels);
                    }
                }
            }
        }
        OutputFormat::Name => {
            for row in &rows {
                println!("network/{}", row.name);
            }
        }
    }

    if let Some(sort_field) = sort_by {
        let mut json_rows: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| serde_json::to_value(r).unwrap_or_default())
            .collect();
        sort_by_path(&mut json_rows, &sort_field);
        print_json(&json_rows)?;
    }

    Ok(())
}

async fn get_network(
    client: &DockerClient,
    name: &str,
    output: OutputFormat,
) -> anyhow::Result<()> {
    let networks = crate::api::network::list_networks(client.inner()).await?;
    let network = networks
        .into_iter()
        .find(|n| {
            n.name.as_ref() == Some(&name.to_string()) || n.id.as_ref() == Some(&name.to_string())
        })
        .ok_or_else(|| anyhow::anyhow!("Network {} not found", name))?;

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
    show_labels: bool,
    selector: Option<String>,
    field_selector: Option<String>,
    sort_by: Option<String>,
    _watch: bool,
) -> anyhow::Result<()> {
    let secrets = crate::api::secret::list_secrets(client.inner()).await?;

    let rows: Vec<crate::models::secret::SecretRow> = secrets
        .into_iter()
        .filter(|s| {
            selector
                .as_ref()
                .map(|sel| {
                    matches_selector(&s.spec.as_ref().and_then(|spec| spec.labels.clone()), sel)
                })
                .unwrap_or(true)
        })
        .filter(|s| {
            field_selector
                .as_ref()
                .map(|sel| {
                    let json = serde_json::to_value(s).unwrap_or_default();
                    matches_field_selector(&json, sel)
                })
                .unwrap_or(true)
        })
        .map(|s| {
            let labels = s.spec.as_ref().and_then(|spec| spec.labels.clone());
            crate::models::secret::SecretRow {
                id: s.id.unwrap_or_default(),
                name: s.spec.unwrap_or_default().name.unwrap_or_default(),
                created_at: s.created_at.unwrap_or_default(),
                labels: format_labels(&labels),
            }
        })
        .collect();

    match output {
        OutputFormat::Table => {
            print_table(&rows);
            if show_labels {
                for row in &rows {
                    if !row.labels.is_empty() {
                        println!("  Labels: {}", row.labels);
                    }
                }
            }
        }
        OutputFormat::Json => print_json(&rows)?,
        OutputFormat::Yaml => print_yaml(&rows)?,
        OutputFormat::Wide => {
            print_table(&rows);
            if show_labels {
                for row in &rows {
                    if !row.labels.is_empty() {
                        println!("  Labels: {}", row.labels);
                    }
                }
            }
        }
        OutputFormat::Name => {
            for row in &rows {
                println!("secret/{}", row.name);
            }
        }
    }

    if let Some(sort_field) = sort_by {
        let mut json_rows: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| serde_json::to_value(r).unwrap_or_default())
            .collect();
        sort_by_path(&mut json_rows, &sort_field);
        print_json(&json_rows)?;
    }

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
    show_labels: bool,
    selector: Option<String>,
    field_selector: Option<String>,
    sort_by: Option<String>,
    _watch: bool,
) -> anyhow::Result<()> {
    let configs = crate::api::config::list_configs(client.inner()).await?;

    let rows: Vec<crate::models::config::ConfigRow> = configs
        .into_iter()
        .filter(|c| {
            selector
                .as_ref()
                .map(|sel| {
                    matches_selector(&c.spec.as_ref().and_then(|spec| spec.labels.clone()), sel)
                })
                .unwrap_or(true)
        })
        .filter(|c| {
            field_selector
                .as_ref()
                .map(|sel| {
                    let json = serde_json::to_value(c).unwrap_or_default();
                    matches_field_selector(&json, sel)
                })
                .unwrap_or(true)
        })
        .map(|c| {
            let labels = c.spec.as_ref().and_then(|spec| spec.labels.clone());
            crate::models::config::ConfigRow {
                id: c.id.unwrap_or_default(),
                name: c.spec.unwrap_or_default().name.unwrap_or_default(),
                created_at: c.created_at.unwrap_or_default(),
                labels: format_labels(&labels),
            }
        })
        .collect();

    match output {
        OutputFormat::Table => {
            print_table(&rows);
            if show_labels {
                for row in &rows {
                    if !row.labels.is_empty() {
                        println!("  Labels: {}", row.labels);
                    }
                }
            }
        }
        OutputFormat::Json => print_json(&rows)?,
        OutputFormat::Yaml => print_yaml(&rows)?,
        OutputFormat::Wide => {
            print_table(&rows);
            if show_labels {
                for row in &rows {
                    if !row.labels.is_empty() {
                        println!("  Labels: {}", row.labels);
                    }
                }
            }
        }
        OutputFormat::Name => {
            for row in &rows {
                println!("config/{}", row.name);
            }
        }
    }

    if let Some(sort_field) = sort_by {
        let mut json_rows: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| serde_json::to_value(r).unwrap_or_default())
            .collect();
        sort_by_path(&mut json_rows, &sort_field);
        print_json(&json_rows)?;
    }

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

async fn get_stack(
    client: &DockerClient,
    name: &str,
    output: OutputFormat,
) -> anyhow::Result<()> {
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
