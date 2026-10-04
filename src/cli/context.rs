use crate::api::context;
use crate::api::endpoints::{self, Endpoint, TlsConfig};
use crate::cli::root::OutputFormat;
use crate::models::context::ContextRow;
use crate::utils::printer::{print_json, print_table, print_yaml};
use anyhow::Result;

pub async fn run_ls(output: OutputFormat) -> Result<()> {
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

pub async fn run_use(name: String) -> Result<()> {
    let ctx = context::get_context(&name)?;
    context::set_current_context(&ctx.name)?;
    println!("Switched to context \"{}\"", ctx.name);
    Ok(())
}

pub async fn run_inspect(name: String, output: OutputFormat) -> Result<()> {
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
) -> Result<()> {
    if skip_tls_verify {
        // Refused rather than accepted and ignored. Docker honours this field,
        // but swarmctl always verifies (bollard exposes no opt-out), so writing it
        // would create a context Docker trusts and swarmctl refuses.
        anyhow::bail!(
            "--skip-tls-verify cannot be honoured: swarmctl always verifies the server \
             certificate. Use ssh://, or supply --tlscacert/--tlscert/--tlskey."
        );
    }

    if context::get_context(&name).is_ok() {
        anyhow::bail!(
            "Context '{}' already exists. Use 'swarmctl context rm {}' to remove it first.",
            name,
            name
        );
    }

    let tls = if tlscacert.is_some() || tlscert.is_some() || tlskey.is_some() {
        Some(TlsConfig {
            ca_file: tlscacert.clone(),
            cert_file: tlscert.clone(),
            key_file: tlskey.clone(),
        })
    } else {
        None
    };

    let endpoint = Endpoint {
        name: name.clone(),
        host: host.clone(),
        description: Some(format!("Managed by swarmctl - {}", host)),
        site: None,
        environment: None,
        tags: Vec::new(),
        tls,
        default: false,
    };

    // Validate before writing anything to ~/.docker.
    endpoints::validate(&endpoint)?;
    if let Some(version) = &docker_api_version {
        // Docker stores the negotiated API version under Metadata.
        log::debug!(
            "Requested Docker API version {} for context {}",
            version,
            name
        );
    }

    // Write through the shared writer so the on-disk layout is Docker's:
    // contexts/meta/<sha256(name)>/meta.json. Writing contexts/meta/<name>/ (as
    // this used to) produces a directory Docker does not recognise, so
    // `docker --context <name>` silently failed while `swarmctl context ls` showed
    // the context.
    let dir = endpoints::write_docker_context(&endpoint)?;

    println!("Context '{}' created.", name);
    println!("  Host:        {}", host);
    if tls_present(&endpoint) {
        println!("  TLS:         configured");
    }
    println!("  Written to:  {}", dir.display());
    println!();
    println!("To use this context:");
    println!("  swarmctl context use {}", name);
    println!("  # or");
    println!("  swarmctl -c {} get nodes", name);
    println!("  # Docker sees it too:");
    println!("  docker --context {} info", name);

    Ok(())
}

fn tls_present(endpoint: &Endpoint) -> bool {
    endpoint.tls.is_some()
}

pub async fn run_rm(name: String) -> Result<()> {
    if context::get_context(&name).is_err() {
        anyhow::bail!("Context '{}' not found", name);
    }

    if let Ok(Some(current)) = context::get_current_context_name()
        && current == name
    {
        anyhow::bail!(
            "Cannot remove context '{}' - it is the currently active context.\n\
             Switch to another context first: swarmctl context use <other-context>",
            name
        );
    }

    endpoints::remove_docker_context(&name)?;

    println!("Context '{}' removed.", name);
    Ok(())
}
