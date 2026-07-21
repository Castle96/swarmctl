use crate::api::client::DockerClient;
use bollard::query_parameters::TopOptions;
use futures::StreamExt;

pub async fn run_stats(
    client: &DockerClient,
    service_filter: Option<String>,
) -> anyhow::Result<()> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;

    let running_tasks: Vec<_> = tasks
        .into_iter()
        .filter(|t| {
            let desired = t
                .desired_state
                .unwrap_or(bollard::models::TaskState::RUNNING);
            let is_running = matches!(desired, bollard::models::TaskState::RUNNING);
            let service_match = if let Some(ref filter) = service_filter {
                t.service_id.as_deref() == Some(filter)
                    || t.name
                        .as_deref()
                        .map(|n| n.contains(filter))
                        .unwrap_or(false)
            } else {
                true
            };
            is_running && service_match
        })
        .collect();

    if running_tasks.is_empty() {
        anyhow::bail!("No running tasks found");
    }

    println!(
        "{:<20} {:<8} {:<14} {:<14} {:<18}",
        "CONTAINER ID", "CPU %", "MEM USAGE", "MEM LIMIT", "NET I/O"
    );
    println!("{}", "-".repeat(76));

    for task in &running_tasks {
        let container_id = task
            .status
            .as_ref()
            .and_then(|s| s.container_status.as_ref())
            .and_then(|cs| cs.container_id.as_deref())
            .unwrap_or("");

        if container_id.is_empty() {
            continue;
        }

        let short_id = if container_id.len() > 12 {
            &container_id[..12]
        } else {
            container_id
        };

        let options = bollard::query_parameters::StatsOptionsBuilder::default()
            .stream(false)
            .build();
        let mut stream = client.inner().stats(container_id, Some(options)).take(1);

        if let Some(result) = stream.next().await {
            match result {
                Ok(stats) => {
                    let cpu = cpu_percent(&stats);
                    let mem_usage = stats
                        .memory_stats
                        .as_ref()
                        .and_then(|m| m.usage)
                        .unwrap_or(0);
                    let mem_limit = stats
                        .memory_stats
                        .as_ref()
                        .and_then(|m| m.limit)
                        .unwrap_or(0);
                    let (rx, tx) = network_io(&stats);

                    println!(
                        "{:<20} {:<8.1} {:<14} {:<14} {:<18}",
                        short_id,
                        cpu,
                        format_bytes(mem_usage),
                        format_bytes(mem_limit),
                        format!("{} / {}", format_bytes(rx), format_bytes(tx)),
                    );
                }
                Err(e) => {
                    eprintln!("Error getting stats for {}: {}", short_id, e);
                }
            }
        }
    }

    Ok(())
}

fn cpu_percent(stats: &bollard::models::ContainerStatsResponse) -> f64 {
    let cpu = stats.cpu_stats.as_ref();
    let precpu = stats.precpu_stats.as_ref();
    let cpu_delta = cpu
        .and_then(|c| c.cpu_usage.as_ref())
        .and_then(|u| u.total_usage)
        .unwrap_or(0)
        .saturating_sub(
            precpu
                .and_then(|c| c.cpu_usage.as_ref())
                .and_then(|u| u.total_usage)
                .unwrap_or(0),
        );
    let system_delta = cpu
        .and_then(|c| c.system_cpu_usage)
        .unwrap_or(0)
        .saturating_sub(precpu.and_then(|c| c.system_cpu_usage).unwrap_or(0));
    let online_cpus = cpu.and_then(|c| c.online_cpus).unwrap_or(1) as f64;
    if system_delta > 0 && cpu_delta > 0 {
        (cpu_delta as f64 / system_delta as f64) * online_cpus * 100.0
    } else {
        0.0
    }
}

fn network_io(stats: &bollard::models::ContainerStatsResponse) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    if let Some(ref networks) = stats.networks {
        for net in networks.values() {
            rx += net.rx_bytes.unwrap_or(0);
            tx += net.tx_bytes.unwrap_or(0);
        }
    }
    (rx, tx)
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut i = 0;
    while size >= 1024.0 && i < UNITS.len() - 1 {
        size /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{}B", bytes)
    } else {
        format!("{:.1}{}", size, UNITS[i])
    }
}

pub async fn run_service(
    client: &DockerClient,
    service_name: String,
    ps_args: Vec<String>,
) -> anyhow::Result<()> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;
    let running_tasks: Vec<_> = tasks
        .into_iter()
        .filter(|t| {
            let desired = t
                .desired_state
                .unwrap_or(bollard::models::TaskState::RUNNING);
            let is_running = matches!(desired, bollard::models::TaskState::RUNNING);
            let service_match = t.service_id.as_deref() == Some(&service_name);
            is_running && service_match
        })
        .collect();

    if running_tasks.is_empty() {
        return Err(anyhow::anyhow!(
            "No running tasks found for service '{}'",
            service_name
        ));
    }

    let ps_arg = if ps_args.is_empty() {
        "-ef".to_string()
    } else {
        ps_args.join(" ")
    };

    for task in &running_tasks {
        let task_id = task.id.as_deref().unwrap_or("unknown");
        let container_id = task
            .status
            .as_ref()
            .and_then(|s| s.container_status.as_ref())
            .and_then(|cs| cs.container_id.as_deref())
            .unwrap_or("")
            .to_string();

        if container_id.is_empty() {
            println!("Task {}: no container ID", task_id);
            continue;
        }

        println!("=== Task {} (container: {}) ===", task_id, container_id);
        let options = TopOptions {
            ps_args: ps_arg.clone(),
        };
        match client
            .inner()
            .top_processes(&container_id, Some(options))
            .await
        {
            Ok(top) => {
                if let Some(titles) = &top.titles {
                    println!("{}", titles.join("\t"));
                }
                if let Some(processes) = &top.processes {
                    for proc in processes {
                        println!("{}", proc.join("\t"));
                    }
                }
            }
            Err(e) => {
                eprintln!("Error getting processes for {}: {}", container_id, e);
            }
        }
        println!();
    }

    Ok(())
}

pub async fn run_node(client: &DockerClient, name: Option<String>) -> anyhow::Result<()> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;

    let filtered: Vec<_> = if let Some(ref n) = name {
        nodes
            .into_iter()
            .filter(|node| node.spec.as_ref().and_then(|s| s.name.as_ref()) == Some(n))
            .collect()
    } else {
        nodes
    };

    if filtered.is_empty() {
        if let Some(ref n) = name {
            anyhow::bail!("Node '{}' not found", n);
        }
        println!("No nodes found");
        return Ok(());
    }

    println!(
        "{:<20} {:<12} {:<10} {:<10} {:<10}",
        "NAME", "STATUS", "CPUS", "MEMORY", "AVAILABILITY"
    );
    println!("{}", "-".repeat(62));

    for node in &filtered {
        let node_name = node
            .spec
            .as_ref()
            .and_then(|s| s.name.as_ref())
            .map(|s| s.as_str())
            .unwrap_or("-");

        let status = node
            .status
            .as_ref()
            .and_then(|s| s.state.as_ref())
            .map(|s| format!("{:?}", s))
            .unwrap_or_else(|| "-".to_string());

        let availability = node
            .spec
            .as_ref()
            .and_then(|s| s.availability.as_ref())
            .map(|a| a.as_ref().to_string())
            .unwrap_or_else(|| "-".to_string());

        let cpus = node
            .description
            .as_ref()
            .and_then(|d| d.resources.as_ref())
            .and_then(|r| r.nano_cpus)
            .map(|n| format!("{:.1}", n as f64 / 1_000_000_000.0))
            .unwrap_or_else(|| "-".to_string());

        let mem = node
            .description
            .as_ref()
            .and_then(|d| d.resources.as_ref())
            .and_then(|r| r.memory_bytes)
            .map(|b| {
                if b >= 1_073_741_824 {
                    format!("{:.1}GiB", b as f64 / 1_073_741_824.0)
                } else if b >= 1_048_576 {
                    format!("{:.1}MiB", b as f64 / 1_048_576.0)
                } else {
                    format!("{}B", b)
                }
            })
            .unwrap_or_else(|| "-".to_string());

        println!(
            "{:<20} {:<12} {:<10} {:<10} {:<10}",
            node_name, status, cpus, mem, availability
        );
    }

    Ok(())
}
