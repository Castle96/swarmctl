use crate::vault::LocalVault;
use anyhow::Result;

pub async fn run_init() -> Result<()> {
    if LocalVault::exists() {
        println!("Vault already exists at ~/.swarmctl/vault.json");
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
    println!("  Location: ~/.swarmctl/vault.json");
    println!("  Created:  {}", status.created_at);

    Ok(())
}

pub async fn run_status() -> Result<()> {
    if !LocalVault::exists() {
        println!("No vault found at ~/.swarmctl/vault.json");
        println!("Run `swarmctl vault init` to create one.");
        return Ok(());
    }

    let password = rpassword::prompt_password("Vault password: ")?;
    let vault = LocalVault::open(&password)?;
    let status = vault.status();

    println!();
    println!("Vault Status");
    println!("═══════════════════════════════════════");
    println!("  Location:     ~/.swarmctl/vault.json");
    println!("  Created:      {}", status.created_at);
    println!("  Swarm Name:   {}", if status.swarm_name.is_empty() { "(none)".to_string() } else { status.swarm_name });
    println!("  Has Tokens:   {}", if status.has_tokens { "yes" } else { "no" });
    println!("  Has Unlock:   {}", if status.has_unlock_key { "yes" } else { "no" });
    println!("  Nodes Tracked: {}", status.node_count);

    if let Some(data) = vault.data() {
        if !data.nodes.is_empty() {
            println!();
            println!("  Tracked Nodes:");
            for node in &data.nodes {
                println!("    - {} ({}) [{}]", node.hostname, node.role, &node.node_id[..12.min(node.node_id.len())]);
            }
        }
    }

    println!();
    Ok(())
}

pub async fn run_unlock() -> Result<()> {
    if !LocalVault::exists() {
        println!("No vault found at ~/.swarmctl/vault.json");
        println!("Run `swarmctl vault init` to create one.");
        return Ok(());
    }

    let password = rpassword::prompt_password("Vault password: ")?;
    let vault = LocalVault::open(&password)?;

    println!();
    println!("Vault unlocked.");

    if let Some(data) = vault.data() {
        if !data.join_tokens.worker.is_empty() {
            println!("  Worker token available");
        }
        if !data.join_tokens.manager.is_empty() {
            println!("  Manager token available");
        }
        if data.nodes.is_empty() {
            println!("  No nodes tracked");
        } else {
            println!("  {} node(s) tracked", data.nodes.len());
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
