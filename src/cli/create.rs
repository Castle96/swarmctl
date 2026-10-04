use crate::api::client::DockerClient;
use crate::cli::root::{OutputFormat, ResourceType};
use crate::utils::printer::report_created;
use std::io::{self, Read};

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    output: OutputFormat,
) -> anyhow::Result<()> {
    match resource {
        ResourceType::Services => {
            create_service(client, name, filename, stdin, dry_run, output).await?
        }
        ResourceType::Networks => {
            create_network(client, name, filename, stdin, dry_run, output).await?
        }
        ResourceType::Secrets => {
            create_secret(client, name, filename, stdin, dry_run, output).await?
        }
        ResourceType::Configs => {
            create_config(client, name, filename, stdin, dry_run, output).await?
        }
        _ => {
            return Err(anyhow::anyhow!(
                "Creating {} is not yet supported",
                format!("{:?}", resource).to_lowercase()
            ));
        }
    }

    Ok(())
}

async fn create_service(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    output: OutputFormat,
) -> anyhow::Result<()> {
    let spec_content = if stdin {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer
    } else if let Some(filename) = filename {
        std::fs::read_to_string(&filename)?
    } else {
        return Err(anyhow::anyhow!("Must specify --filename or use --stdin"));
    };

    let spec: bollard::models::ServiceSpec = serde_json::from_str(&spec_content)?;

    let mut final_spec = spec;
    if let Some(name) = name {
        final_spec.name = Some(name);
    }
    let final_name = final_spec.name.clone().unwrap_or_default();

    if dry_run {
        println!("[dry run] Would create service '{}'", final_name);
        return Ok(());
    }

    let response = client.inner().create_service(final_spec, None).await?;
    report_created("Service", &final_name, &response, &output)?;

    Ok(())
}

async fn create_network(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    output: OutputFormat,
) -> anyhow::Result<()> {
    let spec_content = if stdin {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        buffer
    } else if let Some(filename) = filename {
        std::fs::read_to_string(&filename)?
    } else {
        return Err(anyhow::anyhow!("Must specify --filename or use --stdin"));
    };

    let spec: bollard::models::NetworkCreateRequest = serde_json::from_str(&spec_content)?;

    let mut final_spec = spec;
    if let Some(name) = name {
        final_spec.name = name;
    }
    let final_name = final_spec.name.clone();

    if dry_run {
        println!("[dry run] Would create network '{}'", final_name);
        return Ok(());
    }

    let response = client.inner().create_network(final_spec).await?;
    report_created("Network", &final_name, &response, &output)?;

    Ok(())
}

async fn create_secret(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    output: OutputFormat,
) -> anyhow::Result<()> {
    let data = if stdin {
        let mut buffer = Vec::new();
        io::stdin().read_to_end(&mut buffer)?;
        buffer
    } else if let Some(filename) = filename {
        std::fs::read(&filename)?
    } else {
        return Err(anyhow::anyhow!("Must specify --filename or use --stdin"));
    };

    let secret_name = name.ok_or_else(|| anyhow::anyhow!("Secret name is required"))?;

    if dry_run {
        println!("[dry run] Would create secret '{}'", secret_name);
        return Ok(());
    }

    let data_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);

    let spec = bollard::models::SecretSpec {
        name: Some(secret_name.clone()),
        data: Some(data_b64),
        ..Default::default()
    };

    let response = client.inner().create_secret(spec).await?;
    report_created("Secret", &secret_name, &response, &output)?;

    Ok(())
}

async fn create_config(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    output: OutputFormat,
) -> anyhow::Result<()> {
    let data = if stdin {
        let mut buffer = Vec::new();
        io::stdin().read_to_end(&mut buffer)?;
        buffer
    } else if let Some(filename) = filename {
        std::fs::read(&filename)?
    } else {
        return Err(anyhow::anyhow!("Must specify --filename or use --stdin"));
    };

    let config_name = name.ok_or_else(|| anyhow::anyhow!("Config name is required"))?;

    if dry_run {
        println!("[dry run] Would create config '{}'", config_name);
        return Ok(());
    }

    let data_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);

    let spec = bollard::models::ConfigSpec {
        name: Some(config_name.clone()),
        data: Some(data_b64),
        ..Default::default()
    };

    let response = client.inner().create_config(spec).await?;
    report_created("Config", &config_name, &response, &output)?;

    Ok(())
}
