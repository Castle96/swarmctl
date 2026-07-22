use crate::api::context;
use crate::cli::root::OutputFormat;
use crate::models::context::ContextRow;
use crate::utils::printer::{print_json, print_table, print_yaml};
use anyhow::Context;
use std::collections::HashMap;
use std::fs;

pub async fn run_ls(output: OutputFormat) -> anyhow::Result<()> {
    let contexts = context::list_contexts()?;

    let rows: Vec<ContextRow> = contexts
        .into_iter()
        .map(|c| ContextRow {
            name: c.name,
            description: c.description,
            host: c.host,
            current: c.is_current,
        })
        .collect();

    match output {
        OutputFormat::Table => print_table(&rows),
        OutputFormat::Json => print_json(&rows)?,
        OutputFormat::Yaml => print_yaml(&rows)?,
        _ => print_table(&rows),
    }

    Ok(())
}

pub async fn run_use(name: String) -> anyhow::Result<()> {
    let ctx = context::get_context(&name)?;
    context::set_current_context(&ctx.name)?;
    println!("Switched to context \"{}\"", ctx.name);
    Ok(())
}

pub async fn run_inspect(name: String, output: OutputFormat) -> anyhow::Result<()> {
    let ctx = context::get_context(&name)?;

    match output {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&ctx)?;
            println!("{}", json);
        }
        OutputFormat::Yaml => {
            let yaml = serde_yaml::to_string(&ctx)?;
            println!("{}", yaml);
        }
        _ => {
            println!("Name:        {}", ctx.name);
            println!("Description: {}", ctx.description);
            println!("Host:        {}", ctx.host);
            println!("Current:     {}", ctx.is_current);
            if let Some(tls) = &ctx.tls {
                println!("TLS:");
                if let Some(ca) = &tls.ca_file {
                    println!("  CA File:   {}", ca);
                }
                if let Some(cert) = &tls.cert_file {
                    println!("  Cert File: {}", cert);
                }
                if let Some(key) = &tls.key_file {
                    println!("  Key File:  {}", key);
                }
            }
        }
    }

    Ok(())
}

pub async fn run_create(
    name: String,
    host: String,
    docker_api_version: Option<String>,
    skip_tls_verify: bool,
    tlscacert: Option<String>,
    tlscert: Option<String>,
    tlskey: Option<String>,
) -> anyhow::Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let context_dir = std::path::PathBuf::from(&home)
        .join(".docker")
        .join("contexts")
        .join("meta")
        .join(&name);

    if context_dir.exists() {
        anyhow::bail!("Context '{}' already exists. Use 'swarmctl context rm {}' to remove it first.", name, name);
    }

    fs::create_dir_all(&context_dir)
        .with_context(|| format!("Failed to create context directory: {}", context_dir.display()))?;

    let mut endpoints = HashMap::new();
    let mut docker_endpoint = serde_json::json!({
        "Host": host,
    });

    if skip_tls_verify {
        docker_endpoint["SkipTLSVerify"] = serde_json::json!(true);
    }

    if let Some(ref ca) = tlscacert {
        docker_endpoint["TLS"] = serde_json::json!({
            "CAFile": ca,
            "CertFile": tlscert.as_deref().unwrap_or(""),
            "KeyFile": tlskey.as_deref().unwrap_or(""),
        });
    }

    endpoints.insert("docker".to_string(), docker_endpoint);

    let mut meta = serde_json::json!({
        "Name": name,
        "Metadata": {
            "Description": format!("Managed by swarmctl - {}", host),
        },
        "Endpoints": endpoints,
    });

    if let Some(version) = docker_api_version {
        meta["Metadata"]["dockerAPIVersion"] = serde_json::json!(version);
    }

    let meta_path = context_dir.join("meta.json");
    let content = serde_json::to_string_pretty(&meta)?;
    fs::write(&meta_path, content)
        .with_context(|| format!("Failed to write {}", meta_path.display()))?;

    println!("Context '{}' created.", name);
    println!("  Host: {}", host);
    if tlscacert.is_some() || tlscert.is_some() || tlskey.is_some() {
        println!("  TLS configured");
    }
    println!();
    println!("To use this context:");
    println!("  swarmctl context use {}", name);
    println!("  # or");
    println!("  swarmctl -c {} get nodes", name);

    Ok(())
}

pub async fn run_rm(name: String) -> anyhow::Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let context_dir = std::path::PathBuf::from(&home)
        .join(".docker")
        .join("contexts")
        .join("meta")
        .join(&name);

    if !context_dir.exists() {
        anyhow::bail!("Context '{}' not found", name);
    }

    // Check if this is the currently active context
    if let Ok(Some(current)) = context::get_current_context_name() {
        if current == name {
            anyhow::bail!(
                "Cannot remove context '{}' - it is the currently active context.\n\
                 Switch to another context first: swarmctl context use <other-context>",
                name
            );
        }
    }

    fs::remove_dir_all(&context_dir)
        .with_context(|| format!("Failed to remove context directory: {}", context_dir.display()))?;

    println!("Context '{}' removed.", name);
    Ok(())
}
