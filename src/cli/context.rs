use crate::api::context;
use crate::cli::root::OutputFormat;
use crate::models::context::ContextRow;
use crate::utils::printer::{print_json, print_table, print_yaml};

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
