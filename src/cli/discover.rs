use crate::api::client::DockerClient;
use crate::api::discovery::{self};
use anyhow::Result;

pub async fn run_scan(
    _client: &DockerClient,
    subnet: Option<String>,
    json_output: bool,
) -> Result<()> {
    // Progress messages go to stderr so `--json` output is pipeable into jq.
    eprintln!("Scanning network for Docker hosts...");

    let hosts = discovery::scan_subnet(subnet.as_deref()).await?;

    if hosts.is_empty() {
        if json_output {
            println!("[]");
        } else {
            println!("No Docker hosts found on the network.");
        }
        return Ok(());
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&hosts)?);
    } else {
        println!("Found {} Docker host(s):\n", hosts.len());
        for (i, host) in hosts.iter().enumerate() {
            let swarm_badge = if host.swarm_active { " [SWARM]" } else { "" };
            let role_badge = if host.is_manager {
                " (manager)"
            } else if host.swarm_active {
                " (worker)"
            } else {
                ""
            };
            let port_info = match (host.docker_port, host.swarm_port) {
                (Some(dp), Some(sp)) => format!("docker:{}, swarm:{}", dp, sp),
                (Some(dp), None) => format!("docker:{}", dp),
                (None, Some(sp)) => format!("swarm:{}", sp),
                (None, None) => "unknown ports".to_string(),
            };

            println!(
                "  [{}] {}{}{}\n      {}\n      {}\n",
                i + 1,
                host.ip,
                host.hostname
                    .as_deref()
                    .map(|h| format!(" ({})", h))
                    .unwrap_or_default(),
                swarm_badge,
                port_info,
                host.docker_version
                    .as_deref()
                    .map(|v| format!("Docker {}", v))
                    .unwrap_or_else(|| "Docker version unknown".to_string())
                    .to_string()
                    + role_badge,
            );

            if host.swarm_active {
                if let Some(ref name) = host.swarm_name {
                    println!("      Swarm: {}", name);
                }
                if let (Some(managers), Some(total)) = (host.manager_count, host.node_count) {
                    println!("      Nodes: {} total, {} manager(s)", total, managers);
                }
            }
        }
    }

    Ok(())
}

pub async fn run_interactive(client: &DockerClient, subnet: Option<String>) -> Result<()> {
    println!("Scanning network for Docker hosts...\n");

    let hosts = discovery::scan_subnet(subnet.as_deref()).await?;

    if hosts.is_empty() {
        println!("No Docker hosts found on the network.");
        println!();
        println!("Would you like to initialize a new swarm on this node? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim().eq_ignore_ascii_case("y") {
            return run_create_new(client).await;
        }
        return Ok(());
    }

    println!("Found {} Docker host(s):\n", hosts.len());

    for (i, host) in hosts.iter().enumerate() {
        let swarm_badge = if host.swarm_active { " *" } else { "" };
        let role = if host.is_manager {
            "manager"
        } else if host.swarm_active {
            "worker"
        } else {
            "standalone"
        };
        let name = host.hostname.as_deref().unwrap_or(&host.ip);

        println!(
            "  [{}] {} ({}){} - {}",
            i + 1,
            host.ip,
            name,
            swarm_badge,
            role
        );

        if host.swarm_active {
            if let Some(ref swarm_name) = host.swarm_name {
                println!("       Swarm: {}", swarm_name);
            }
            if let (Some(m), Some(n)) = (host.manager_count, host.node_count) {
                println!("       Nodes: {} total, {} manager(s)", n, m);
            }
        }
    }

    println!();
    println!("  [0] Create a new swarm on this node");
    println!();
    println!("Enter choice (1-{} or 0 to create new): ", hosts.len());

    let Some(choice) = read_choice()? else {
        println!("Cancelled.");
        return Ok(());
    };
    if choice == 0 {
        return run_create_new(client).await;
    }

    if choice < 1 || choice > hosts.len() {
        anyhow::bail!("Invalid selection: {}", choice);
    }

    let selected = &hosts[choice - 1];

    if !selected.swarm_active {
        println!();
        println!(
            "Host {} is not part of a swarm. Choose a host marked with * (swarm active).",
            selected.ip
        );
        if read_confirm("Would you like to create a new swarm on this node instead?")? {
            return run_create_new(client).await;
        }
        anyhow::bail!("Cannot join: target host has no active swarm.");
    }

    let mut selected_host = selected.clone();
    println!();
    println!(
        "Connecting to manager at {} to retrieve join tokens...",
        selected_host.ip
    );

    if selected_host.join_token_worker.is_none() {
        discovery::probe_host_for_tokens(&mut selected_host).await?;
    }

    let worker_token = selected_host.join_token_worker.as_deref().ok_or_else(|| {
        anyhow::anyhow!("Failed to retrieve join token from {}", selected_host.ip)
    })?;

    let manager_token = selected_host.join_token_manager.as_deref();

    println!();
    println!("Join tokens retrieved from {}: ", selected_host.ip);
    println!(
        "  Worker token:  {}...",
        &worker_token[..24.min(worker_token.len())]
    );
    if let Some(mt) = manager_token {
        println!("  Manager token: {}...", &mt[..24.min(mt.len())]);
    }

    println!();
    println!("Join as:");
    println!("  [1] Worker");
    if manager_token.is_some() {
        println!("  [2] Manager");
    }
    println!();
    print!("Choice: ");
    use std::io::Write;
    std::io::stdout().flush()?;

    let mut role_input = String::new();
    std::io::stdin().read_line(&mut role_input)?;
    let role_choice: u32 = match role_input.trim() {
        "" | "1" => 1,
        "2" => 2,
        other => anyhow::bail!(
            "Invalid role '{}'. Expected 1 (worker) or 2 (manager).",
            other
        ),
    };

    let token = match (role_choice, manager_token) {
        (2, Some(token)) => token,
        _ => worker_token,
    };

    println!();
    println!("Detecting local IP for advertise address...");
    let advertise_addr = discovery::detect_local_ip()?.to_string();
    println!("  Advertise address: {}", advertise_addr);

    println!();
    println!(
        "Joining swarm at {} (manager: {})...",
        selected_host.ip,
        selected_host.swarm_name.as_deref().unwrap_or("unnamed")
    );

    crate::api::swarm::join_swarm(
        client.inner(),
        &advertise_addr,
        &format!("{}:{}", selected_host.ip, discovery::SWARM_PORT),
        token,
    )
    .await?;

    println!();
    println!("Successfully joined the swarm!");

    if let Ok(vault_password) =
        rpassword::prompt_password("Vault password (to save tokens, or empty to skip): ")
        && !vault_password.is_empty()
    {
        let vault = if crate::vault::LocalVault::exists() {
            crate::vault::LocalVault::open(&vault_password)?
        } else {
            crate::vault::LocalVault::create(&vault_password)?
        };
        let mut vault = vault;
        let host = std::env::var("DOCKER_HOST")
            .unwrap_or_else(|_| "unix:///var/run/docker.sock".to_string());
        let swarm_name = client
            .inner()
            .info()
            .await
            .ok()
            .and_then(|i| i.name)
            .unwrap_or_else(|| "docker".to_string());

        let tokens = crate::vault::models::JoinTokens {
            worker: worker_token.to_string(),
            manager: manager_token.unwrap_or_default().to_string(),
        };
        let endpoint = crate::api::endpoints::current_endpoint_key();
        vault.store_swarm_tokens(&endpoint, tokens, None, &host, &swarm_name)?;
        println!("Tokens saved to vault under '{}'.", endpoint);
    }

    Ok(())
}

async fn run_create_new(client: &DockerClient) -> Result<()> {
    println!();
    println!("Initializing a new swarm on this node...");

    if crate::api::swarm::is_swarm_active(client.inner()).await {
        println!("Swarm is already active on this node.");
        return Ok(());
    }

    let addr = discovery::detect_local_ip()?.to_string();
    println!("  Advertise address: {}", addr);

    let node_id = crate::api::swarm::init_swarm(client.inner(), &addr).await?;
    println!("  Node ID: {}", node_id);

    let tokens = crate::api::swarm::get_join_tokens(client.inner()).await?;

    println!();
    println!("Swarm initialized successfully.");
    println!();
    println!(
        "  Worker join token:  {}...",
        &tokens.worker[..24.min(tokens.worker.len())]
    );
    println!(
        "  Manager join token: {}...",
        &tokens.manager[..24.min(tokens.manager.len())]
    );

    if let Ok(vault_password) =
        rpassword::prompt_password("Vault password (to save tokens, or empty to skip): ")
        && !vault_password.is_empty()
    {
        let vault = if crate::vault::LocalVault::exists() {
            crate::vault::LocalVault::open(&vault_password)?
        } else {
            crate::vault::LocalVault::create(&vault_password)?
        };
        let mut vault = vault;
        let host = std::env::var("DOCKER_HOST")
            .unwrap_or_else(|_| "unix:///var/run/docker.sock".to_string());
        let swarm_name = client
            .inner()
            .info()
            .await
            .ok()
            .and_then(|i| i.name)
            .unwrap_or_else(|| "docker".to_string());
        let endpoint = crate::api::endpoints::current_endpoint_key();
        vault.store_swarm_tokens(&endpoint, tokens, None, &host, &swarm_name)?;
        println!("Tokens saved to vault under '{}'.", endpoint);
    }

    Ok(())
}

/// Read a menu selection.
///
/// Returns `None` on empty input or EOF. Callers must treat that as "cancel"
/// rather than as a selection: option `0` creates a swarm, and pressing Enter at
/// the prompt used to do exactly that.
fn read_choice() -> Result<Option<usize>> {
    use std::io::{self, BufRead, Write};
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    print!("> ");
    stdout.flush()?;
    let mut line = String::new();
    let read = stdin.lock().read_line(&mut line)?;
    if read == 0 {
        // EOF (e.g. piped input): treat as cancel.
        return Ok(None);
    }
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    line.parse::<usize>()
        .map(Some)
        .map_err(|_| anyhow::anyhow!("Invalid input: {}", line))
}

/// Read a yes/no answer, defaulting to no.
fn read_confirm(prompt: &str) -> Result<bool> {
    use std::io::{self, BufRead, Write};
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    print!("{} [y/N] ", prompt);
    stdout.flush()?;
    let mut line = String::new();
    if stdin.lock().read_line(&mut line)? == 0 {
        return Ok(false);
    }
    let answer = line.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
}
