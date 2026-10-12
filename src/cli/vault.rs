use crate::cli::root::OutputFormat;
use crate::utils::printer::{print_json, print_table};
use crate::vault::LocalVault;
use crate::vault::models::JoinTokens;
use anyhow::Result;

const LOCATION: &str = "~/.swarmctl/vault.json";

pub async fn run_init() -> Result<()> {
    if LocalVault::exists() {
        println!("Vault already exists at {}", LOCATION);
        println!("Use `swarmctl vault status` to check its state.");
        return Ok(());
    }

    let password = rpassword::prompt_password("Create vault password: ")?;
    if password.is_empty() {
        anyhow::bail!("Password cannot be empty");
    }

    let confirm = rpassword::prompt_password("Confirm password: ")?;
    if password != confirm {
        anyhow::bail!("Passwords do not match");
    }

    let vault = LocalVault::create(&password)?;
    let status = vault.status();

    println!();
    println!("Vault created successfully.");
    println!("  Location: {}", LOCATION);
    println!("  Created:  {}", status.created_at);
    println!();
    println!("Store credentials for an endpoint with:");
    println!("  swarmctl vault store <endpoint> --worker-token <t> --manager-token <t>");
    println!();
    println!("There is no recovery if the password is lost.");

    Ok(())
}

pub async fn run_status() -> Result<()> {
    if !LocalVault::exists() {
        println!("No vault found at {}", LOCATION);
        println!("Run `swarmctl vault init` to create one.");
        return Ok(());
    }

    let password = rpassword::prompt_password("Vault password: ")?;
    let vault = LocalVault::open(&password)?;
    let status = vault.status();

    println!();
    println!("Vault Status");
    println!("═══════════════════════════════════════");
    println!("  Location:        {}", LOCATION);
    println!("  Created:         {}", status.created_at);
    println!("  Endpoints:       {}", status.total_endpoints);
    println!(
        "  With credentials: {}",
        if status.endpoints.is_empty() {
            "none".to_string()
        } else {
            status.endpoints.join(", ")
        }
    );

    let data = vault.data();
    if !data.has_any() {
        println!();
        println!("  No credentials stored yet.");
        println!("  Add them with:");
        println!("    swarmctl vault store <endpoint> --manager-token <token>");
    } else if !data.endpoints.is_empty() {
        println!();
        println!("  Entries:");
        for (name, entry) in &data.endpoints {
            let tokens = describe_tokens(&entry.join_tokens);
            let key = if entry.unlock_key.is_some() {
                " +unlock"
            } else {
                ""
            };
            let nodes = if entry.nodes.is_empty() {
                String::new()
            } else {
                format!(", {} node(s)", entry.nodes.len())
            };
            println!("    - {:<16} {}{}{}", name, tokens, key, nodes);
        }
    }

    println!();
    Ok(())
}

fn describe_tokens(tokens: &JoinTokens) -> String {
    match (tokens.worker.is_empty(), tokens.manager.is_empty()) {
        (true, true) => "no tokens".to_string(),
        (false, true) => "worker token".to_string(),
        (true, false) => "manager token".to_string(),
        (false, false) => "worker + manager tokens".to_string(),
    }
}

pub async fn run_unlock() -> Result<()> {
    if !LocalVault::exists() {
        println!("No vault found at {}", LOCATION);
        println!("Run `swarmctl vault init` to create one.");
        return Ok(());
    }

    let password = rpassword::prompt_password("Vault password: ")?;
    let vault = LocalVault::open(&password)?;

    println!();
    println!("Vault unlocked.");

    for (name, entry) in &vault.data().endpoints {
        println!("  {}", name);
        println!("    tokens: {}", describe_tokens(&entry.join_tokens));
        if entry.unlock_key.is_some() {
            println!("    unlock key: present");
        }
        if entry.nodes.is_empty() {
            println!("    nodes: none tracked");
        } else {
            println!("    nodes: {} tracked", entry.nodes.len());
            for node in &entry.nodes {
                println!(
                    "      - {} ({}) [{}]",
                    node.hostname,
                    node.role,
                    &node.node_id[..12.min(node.node_id.len())]
                );
            }
        }
    }

    Ok(())
}

pub async fn run_set_key() -> Result<()> {
    if !LocalVault::exists() {
        println!("No vault found. Run `swarmctl vault init` first.");
        return Ok(());
    }

    let current = rpassword::prompt_password("Current vault password: ")?;
    let mut vault = LocalVault::open(&current)?;

    let new_password = rpassword::prompt_password("New vault password: ")?;
    if new_password.is_empty() {
        anyhow::bail!("Password cannot be empty");
    }

    let confirm = rpassword::prompt_password("Confirm new password: ")?;
    if new_password != confirm {
        anyhow::bail!("Passwords do not match");
    }

    vault.change_password(&new_password)?;

    println!("Vault password changed.");
    Ok(())
}

/// List stored credentials per endpoint.
pub async fn run_ls(output: OutputFormat) -> Result<()> {
    if !LocalVault::exists() {
        println!("No vault found at {}", LOCATION);
        return Ok(());
    }

    let password = rpassword::prompt_password("Vault password: ")?;
    let vault = LocalVault::open(&password)?;
    let rows = vault.rows();

    if rows.is_empty() {
        println!("The vault is empty.");
        println!();
        println!("Store credentials with:");
        println!("  swarmctl vault store <endpoint> --manager-token <token>");
        return Ok(());
    }

    match output {
        OutputFormat::Json => print_json(&rows)?,
        _ => print_table(&rows),
    }
    Ok(())
}

/// Store join tokens for an endpoint.
pub async fn run_store(
    endpoint: &str,
    worker_token: Option<&str>,
    manager_token: Option<&str>,
    output: OutputFormat,
) -> Result<()> {
    if !LocalVault::exists() {
        anyhow::bail!("No vault found. Run `swarmctl vault init` first.");
    }
    if worker_token.is_none() && manager_token.is_none() {
        anyhow::bail!("Supply at least one of --worker-token or --manager-token");
    }

    let password = rpassword::prompt_password("Vault password: ")?;
    let mut vault = LocalVault::open(&password)?;

    // Preserve whatever is already stored unless overridden.
    let existing = vault.data().get(endpoint).cloned().unwrap_or_default();
    let tokens = JoinTokens {
        worker: worker_token
            .map(|t| t.to_string())
            .unwrap_or_else(|| existing.join_tokens.worker.clone()),
        manager: manager_token
            .map(|t| t.to_string())
            .unwrap_or_else(|| existing.join_tokens.manager.clone()),
    };
    if tokens.is_empty() {
        anyhow::bail!("Refusing to store an empty token pair for '{}'", endpoint);
    }

    vault.store_swarm_tokens(
        endpoint,
        tokens,
        existing.unlock_key.clone(),
        &existing.docker_host,
        &existing.swarm_name,
    )?;

    println!("Stored credentials for '{}'.", endpoint);
    let rows = vault.rows();
    if matches!(output, OutputFormat::Json) {
        print_json(&rows)?;
    }
    Ok(())
}

/// Remove stored credentials for an endpoint.
pub async fn run_rm(endpoint: &str) -> Result<()> {
    if !LocalVault::exists() {
        anyhow::bail!("No vault found. Run `swarmctl vault init` first.");
    }

    let password = rpassword::prompt_password("Vault password: ")?;
    let mut vault = LocalVault::open(&password)?;
    vault.remove_endpoint(endpoint)?;
    println!("Removed credentials for '{}'.", endpoint);
    Ok(())
}
