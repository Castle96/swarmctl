use crate::api::endpoints::{self, Endpoint, Registry, TlsConfig};
use crate::cli::root::OutputFormat;
use crate::models::endpoint::{EndpointHealthRow, EndpointRow};
use crate::utils::printer::{print_json, print_table, print_yaml};
use anyhow::Result;

fn load() -> Result<Registry> {
    Registry::load()
}

fn save(reg: &Registry) -> Result<()> {
    reg.save()
}

pub async fn run_ls(output: OutputFormat, json_output: bool) -> Result<()> {
    let reg = load()?;
    let current = crate::api::context::get_current_context_name().unwrap_or(None);

    let rows: Vec<EndpointRow> = reg
        .sorted()
        .into_iter()
        .map(|e| EndpointRow {
            name: e.name.clone(),
            host: e.host.clone(),
            environment: e
                .environment
                .clone()
                .unwrap_or_else(|| crate::models::endpoint::NONE_CELL.into()),
            site: e
                .site
                .clone()
                .unwrap_or_else(|| crate::models::endpoint::NONE_CELL.into()),
            tags: if e.tags.is_empty() {
                crate::models::endpoint::NONE_CELL.into()
            } else {
                e.tags.join(",")
            },
            current: current.as_deref() == Some(e.name.as_str()),
            description: e
                .description
                .clone()
                .unwrap_or_else(|| crate::models::endpoint::NONE_CELL.into()),
        })
        .collect();

    if rows.is_empty() {
        if json_output || matches!(output, OutputFormat::Json) {
            println!("[]");
        } else {
            println!("No endpoints registered.");
            println!();
            println!("Add one with:");
            println!(
                "  swarmctl endpoint add prod-eu --host ssh://ops@10.8.0.4 --site eu-west --env prod"
            );
            println!();
            println!("Or import existing Docker contexts:");
            println!("  swarmctl endpoint import");
        }
        return Ok(());
    }

    match output {
        OutputFormat::Json => print_json(&rows)?,
        OutputFormat::Yaml => print_yaml(&rows)?,
        _ => print_table(&rows),
    }
    Ok(())
}

pub async fn run_add(
    name: String,
    host: String,
    description: Option<String>,
    site: Option<String>,
    environment: Option<String>,
    tags: Vec<String>,
    tlscacert: Option<String>,
    tlscert: Option<String>,
    tlskey: Option<String>,
    set_default: bool,
    sync_docker: bool,
) -> Result<()> {
    let mut reg = load()?;

    let tls = if tlscacert.is_some() || tlscert.is_some() || tlskey.is_some() {
        Some(TlsConfig {
            ca_file: tlscacert,
            cert_file: tlscert,
            key_file: tlskey,
        })
    } else {
        None
    };

    // Preserve metadata on replace so `endpoint add` can be used to amend.
    let existing = reg.get(&name).cloned();
    let endpoint = Endpoint {
        name: name.clone(),
        host,
        description: description.or_else(|| existing.as_ref().and_then(|e| e.description.clone())),
        site: site.or_else(|| existing.as_ref().and_then(|e| e.site.clone())),
        environment: environment.or_else(|| existing.as_ref().and_then(|e| e.environment.clone())),
        tags: if tags.is_empty() {
            existing
                .as_ref()
                .map(|e| e.tags.clone())
                .unwrap_or_default()
        } else {
            tags
        },
        tls: tls.or_else(|| existing.as_ref().and_then(|e| e.tls.clone())),
        default: set_default || existing.as_ref().is_some_and(|e| e.default),
    };

    let is_update = existing.is_some();
    reg.upsert(endpoint.clone())?;
    save(&reg)?;

    if sync_docker {
        // A bad TLS path should not leave a broken Docker context behind.
        if let Err(e) = endpoints::write_docker_context(&endpoint) {
            eprintln!("warning: could not create Docker context: {}", e);
        }
    }

    println!(
        "Endpoint '{}' {}.",
        name,
        if is_update { "updated" } else { "created" }
    );
    println!("  Host:        {}", endpoint.host);
    if let Some(env) = &endpoint.environment {
        println!("  Environment: {}", env);
    }
    if let Some(site) = &endpoint.site {
        println!("  Site:        {}", site);
    }
    if endpoint.tls.is_some() {
        println!("  TLS:         configured");
    }
    println!();
    println!("Use it with:");
    println!("  swarmctl -c {} get nodes", name);
    println!("  swarmctl endpoint health --endpoint {}", name);
    Ok(())
}

pub async fn run_rm(name: String, sync_docker: bool) -> Result<()> {
    let mut reg = load()?;
    let removed = reg.remove(&name)?;
    save(&reg)?;

    if sync_docker {
        let _ = endpoints::remove_docker_context(&name);
    }

    println!("Endpoint '{}' removed.", name);
    println!("  Host was: {}", removed.host);
    Ok(())
}

pub async fn run_inspect(name: String, output: OutputFormat) -> Result<()> {
    let reg = load()?;
    let endpoint = reg
        .get(&name)
        .ok_or_else(|| anyhow::anyhow!("Endpoint '{}' not found", name))?;

    match output {
        OutputFormat::Json => print_json(endpoint)?,
        OutputFormat::Yaml => print_yaml(endpoint)?,
        _ => {
            println!("Name:        {}", endpoint.name);
            println!("Host:        {}", endpoint.host);
            println!(
                "Environment: {}",
                endpoint.environment.as_deref().unwrap_or("-")
            );
            println!("Site:        {}", endpoint.site.as_deref().unwrap_or("-"));
            println!(
                "Tags:        {}",
                if endpoint.tags.is_empty() {
                    "-".to_string()
                } else {
                    endpoint.tags.join(", ")
                }
            );
            println!("Default:     {}", endpoint.default);
            println!(
                "Description: {}",
                endpoint.description.as_deref().unwrap_or("-")
            );
            if let Some(tls) = &endpoint.tls {
                println!("TLS:");
                println!("  CA:   {}", tls.ca_file.as_deref().unwrap_or("-"));
                println!("  Cert: {}", tls.cert_file.as_deref().unwrap_or("-"));
                println!("  Key:  {}", tls.key_file.as_deref().unwrap_or("-"));
            }
            println!("Docker digest: {}", endpoints::docker_context_digest(&name));
        }
    }
    Ok(())
}

/// Probe every endpoint, or only those named.
pub async fn run_health(names: Option<String>, output: OutputFormat, watch: bool) -> Result<()> {
    let reg = load()?;

    // Health covers every endpoint unless specific ones are named. Restricting
    // to the fan-out default would hide exactly the endpoints you most want to
    // check.
    let selected: Vec<Endpoint> = match names {
        Some(list) => reg.select(
            &list
                .split(',')
                .map(|s| s.trim().to_string())
                .collect::<Vec<_>>(),
        )?,
        None => reg.sorted().into_iter().cloned().collect(),
    };

    if selected.is_empty() {
        println!("No endpoints registered.");
        return Ok(());
    }

    loop {
        let health = crate::api::health::probe_all(&selected).await;

        let rows: Vec<EndpointHealthRow> = health
            .iter()
            .map(|h| {
                let meta = reg.get(&h.name);
                h.to_row(
                    meta.and_then(|e| e.environment.clone()),
                    meta.and_then(|e| e.site.clone()),
                )
            })
            .collect();

        if matches!(output, OutputFormat::Json) {
            print_json(&health)?;
        } else if matches!(output, OutputFormat::Yaml) {
            print_yaml(&health)?;
        } else if watch {
            // Clear then redraw, so a watch loop does not scroll away.
            print!("\x1b[2J\x1b[H");
            print_table(&rows);
        } else {
            print_table(&rows);

            let unreachable: Vec<&str> = health
                .iter()
                .filter(|h| !h.reachable())
                .map(|h| h.name.as_str())
                .collect();
            if !unreachable.is_empty() {
                println!();
                println!("Unreachable: {}", unreachable.join(", "));
            }
        }

        if !watch {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

pub async fn run_import(remove_missing: bool) -> Result<()> {
    let mut reg = load()?;
    let added = reg.import_docker_contexts()?;
    save(&reg)?;

    if added.is_empty() {
        println!("No new Docker contexts to import.");
        println!();
        println!("Contexts already registered are left untouched.");
        return Ok(());
    }

    println!("Imported {} Docker context(s):", added.len());
    for name in &added {
        let host = reg.get(name).map(|e| e.host.clone()).unwrap_or_default();
        println!("  {} -> {}", name, host);
    }
    println!();
    println!("Annotate them with site and environment:");
    println!(
        "  swarmctl endpoint edit {} --site eu-west --env prod",
        added[0]
    );

    if remove_missing {
        let removed = reg.prune_docker_contexts()?;
        if !removed.is_empty() {
            println!();
            println!(
                "Removed {} stale Docker context(s): {}",
                removed.len(),
                removed.join(", ")
            );
        }
    }
    Ok(())
}

/// Make an endpoint the target for future fan-out.
pub async fn run_use(name: String) -> Result<()> {
    let mut reg = load()?;
    if !reg.endpoints.contains_key(&name) {
        anyhow::bail!(
            "Endpoint '{}' not found. Known endpoints: {}",
            name,
            reg.names().join(", ")
        );
    }
    // Exactly one default.
    for e in reg.endpoints.values_mut() {
        e.default = e.name == name;
    }
    save(&reg)?;

    // Also switch Docker, so `docker` and `swarmctl` agree.
    if let Err(e) = crate::api::context::set_current_context(&name) {
        eprintln!("warning: could not switch Docker context: {}", e);
    }
    if let Err(e) = endpoints::write_docker_context(reg.get(&name).unwrap()) {
        eprintln!("warning: could not create Docker context: {}", e);
    }

    println!("Endpoint '{}' is now the default.", name);
    Ok(())
}

/// Show the Docker context material swarmctl would write.
pub async fn run_sync() -> Result<()> {
    let reg = load()?;
    if reg.endpoints.is_empty() {
        println!("No endpoints registered.");
        return Ok(());
    }
    let written = reg.sync_to_docker()?;
    println!("Wrote {} Docker context(s):", written.len());
    for name in &written {
        println!("  {} ({})", name, endpoints::docker_context_digest(name));
    }
    println!();
    println!("They are usable with docker directly:");
    println!("  docker --context {} info", written[0]);
    Ok(())
}
