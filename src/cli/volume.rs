use crate::api::client::DockerClient;
use crate::cli::root::OutputFormat;
use crate::models::volume::VolumeRow;
use crate::utils::printer::{print_json, print_table, print_yaml};
use std::collections::HashMap;

pub async fn run_ls(client: &DockerClient, output: OutputFormat) -> anyhow::Result<()> {
    let volumes = crate::api::volume::list_volumes(client.inner()).await?;

    let rows: Vec<VolumeRow> = volumes
        .into_iter()
        .map(|v| {
            let labels = v
                .labels
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(", ");

            VolumeRow {
                name: v.name,
                driver: v.driver,
                mountpoint: v.mountpoint,
                labels,
                scope: v.scope
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "local".to_string()),
                created_at: v.created_at
                    .map(|d| format!("{}", d))
                    .unwrap_or_default(),
            }
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

pub async fn run_inspect(client: &DockerClient, name: String, output: OutputFormat) -> anyhow::Result<()> {
    let volume = crate::api::volume::inspect_volume(client.inner(), &name).await?;

    match output {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&volume)?;
            println!("{}", json);
        }
        OutputFormat::Yaml => {
            let yaml = serde_yaml::to_string(&volume)?;
            println!("{}", yaml);
        }
        _ => {
            println!("Name:        {}", volume.name);
            println!("Driver:      {}", volume.driver);
            println!("Mountpoint:  {}", volume.mountpoint);
            println!("Scope:       {}", volume.scope
                .map(|s| s.to_string())
                .unwrap_or_else(|| "local".to_string()));
            if !volume.labels.is_empty() {
                println!("Labels:");
                for (k, v) in &volume.labels {
                    println!("  {}={}", k, v);
                }
            }
            if !volume.options.is_empty() {
                println!("Options:");
                for (k, v) in &volume.options {
                    println!("  {}={}", k, v);
                }
            }
        }
    }

    Ok(())
}

pub async fn run_create(
    client: &DockerClient,
    name: String,
    driver: Option<String>,
    labels: Vec<String>,
) -> anyhow::Result<()> {
    let mut label_map = HashMap::new();
    for label in &labels {
        if let Some((k, v)) = label.split_once('=') {
            label_map.insert(k.to_string(), v.to_string());
        } else {
            label_map.insert(label.to_string(), String::new());
        }
    }

    let driver = driver.unwrap_or_else(|| "local".to_string());

    let volume = crate::api::volume::create_volume(
        client.inner(),
        &name,
        &driver,
        label_map,
    )
    .await?;

    println!("Volume '{}' created.", volume.name);
    println!("  Driver: {}", volume.driver);
    println!("  Mountpoint: {}", volume.mountpoint);

    Ok(())
}

pub async fn run_rm(client: &DockerClient, name: String, force: bool) -> anyhow::Result<()> {
    crate::api::volume::remove_volume(client.inner(), &name, force).await?;
    println!("Volume '{}' removed.", name);
    Ok(())
}
