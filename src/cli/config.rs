use crate::api::client::DockerClient;
use crate::cli::root::OutputFormat;
use crate::utils::printer::{print_json, print_table, print_yaml};
use std::io::{self, Read};

pub async fn run_ls(client: &DockerClient, output: OutputFormat) -> anyhow::Result<()> {
    let configs = crate::api::config::list_configs(client.inner()).await?;

    let rows: Vec<crate::models::config::ConfigRow> = configs
        .into_iter()
        .map(|c| {
            let labels = c
                .spec
                .as_ref()
                .and_then(|spec| spec.labels.clone())
                .map(|l| {
                    l.iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            crate::models::config::ConfigRow {
                id: c.id.unwrap_or_default(),
                name: c.spec.unwrap_or_default().name.unwrap_or_default(),
                created_at: c.created_at.unwrap_or_default(),
                labels,
            }
        })
        .collect();

    match output {
        OutputFormat::Table => print_table(&rows),
        OutputFormat::Json => print_json(&rows)?,
        OutputFormat::Yaml => print_yaml(&rows)?,
        OutputFormat::Wide => print_table(&rows),
        OutputFormat::Name => {
            for row in &rows {
                println!("config/{}", row.name);
            }
        }
    }

    Ok(())
}

pub async fn run_create(
    client: &DockerClient,
    name: String,
    from_file: Option<String>,
    stdin: bool,
) -> anyhow::Result<()> {
    let data = if stdin {
        let mut buffer = Vec::new();
        io::stdin().read_to_end(&mut buffer)?;
        buffer
    } else if let Some(path) = from_file {
        std::fs::read(&path)?
    } else {
        return Err(anyhow::anyhow!("Must specify --from-file or --stdin"));
    };

    let data_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);

    let spec = bollard::models::ConfigSpec {
        name: Some(name.clone()),
        data: Some(data_b64),
        ..Default::default()
    };

    let response = client.inner().create_config(spec).await?;
    println!("Config '{}' created with ID: {}", name, response.id);

    Ok(())
}

pub async fn run_rm(client: &DockerClient, name: String) -> anyhow::Result<()> {
    println!("Removing config '{}'...", name);
    client.inner().delete_config(&name).await?;
    println!("Config '{}' removed", name);
    Ok(())
}

pub async fn run_inspect(
    client: &DockerClient,
    name: String,
    output: OutputFormat,
) -> anyhow::Result<()> {
    let configs = crate::api::config::list_configs(client.inner()).await?;
    let config = configs
        .into_iter()
        .find(|c| c.spec.as_ref().and_then(|s| s.name.as_ref()) == Some(&name))
        .ok_or_else(|| anyhow::anyhow!("Config '{}' not found", name))?;

    match output {
        OutputFormat::Table => {
            let spec = config.spec.as_ref().unwrap();
            println!(
                "Name:\t{}",
                spec.name.as_ref().unwrap_or(&"unknown".to_string())
            );
            println!("ID:\t{}", config.id.unwrap_or_default());
            println!("Created At:\t{}", config.created_at.unwrap_or_default());
            if let Some(labels) = &spec.labels
                && !labels.is_empty()
            {
                println!("Labels:");
                for (k, v) in labels {
                    println!("  {}:\t{}", k, v);
                }
            }
            println!(
                "Data Size:\t{} bytes",
                spec.data.as_ref().map(|d| d.len()).unwrap_or(0)
            );
        }
        OutputFormat::Json => print_json(&config)?,
        OutputFormat::Yaml => print_yaml(&config)?,
        OutputFormat::Wide => {
            let spec = config.spec.as_ref().unwrap();
            println!("Name:\t{}", spec.name.as_deref().unwrap_or("unknown"));
            println!("ID:\t{}", config.id.unwrap_or_default());
        }
        OutputFormat::Name => {
            let spec = config.spec.as_ref().unwrap();
            println!("config/{}", spec.name.as_deref().unwrap_or("unknown"));
        }
    }

    Ok(())
}

pub async fn run_view(client: &DockerClient, name: String) -> anyhow::Result<()> {
    let configs = crate::api::config::list_configs(client.inner()).await?;
    let config = configs
        .into_iter()
        .find(|c| c.spec.as_ref().and_then(|s| s.name.as_ref()) == Some(&name))
        .ok_or_else(|| anyhow::anyhow!("Config '{}' not found", name))?;

    let data_b64 = config
        .spec
        .as_ref()
        .and_then(|s| s.data.as_ref())
        .ok_or_else(|| anyhow::anyhow!("Config '{}' has no data", name))?;

    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data_b64)?;

    let output = String::from_utf8_lossy(&decoded);
    println!("{}", output);

    Ok(())
}
