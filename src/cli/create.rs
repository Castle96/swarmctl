use crate::api::client::DockerClient;
use crate::cli::root::ResourceType;
use std::io::{self, Read};

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
) -> anyhow::Result<()> {
    match resource {
        ResourceType::Services => create_service(client, name, filename, stdin).await?,
        ResourceType::Networks => create_network(client, name, filename, stdin).await?,
        ResourceType::Secrets => create_secret(client, name, filename, stdin).await?,
        ResourceType::Configs => create_config(client, name, filename, stdin).await?,
        _ => return Err(anyhow::anyhow!("Creating {} is not yet supported", format!("{:?}", resource).to_lowercase())),
    }

    Ok(())
}

async fn create_service(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
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

    // Parse the service spec (assuming JSON for now)
    let spec: bollard::models::ServiceSpec = serde_json::from_str(&spec_content)?;

    // Override name if provided
    let mut final_spec = spec;
    if let Some(name) = name {
        final_spec.name = Some(name);
    }

    let response = client.inner().create_service(final_spec, None).await?;
    println!("Service created with ID: {}", response.id.unwrap_or_default());

    Ok(())
}

async fn create_network(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
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

    // Parse the network spec
    let spec: bollard::models::NetworkCreateRequest = serde_json::from_str(&spec_content)?;

    // Override name if provided
    let mut final_spec = spec;
    if let Some(name) = name {
        final_spec.name = name;
    }

    let response = client.inner().create_network(final_spec).await?;
    println!("Network created with ID: {}", response.id);

    Ok(())
}

async fn create_secret(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
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

    let data_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);

    let spec = bollard::models::SecretSpec {
        name: Some(secret_name.clone()),
        data: Some(data_b64),
        ..Default::default()
    };

    let response = client.inner().create_secret(spec).await?;
    println!("Secret {} created with ID: {}", secret_name, response.id);

    Ok(())
}

async fn create_config(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
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

    let data_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);

    let spec = bollard::models::ConfigSpec {
        name: Some(config_name.clone()),
        data: Some(data_b64),
        ..Default::default()
    };

    let response = client.inner().create_config(spec).await?;
    println!("Config {} created with ID: {}", config_name, response.id);

    Ok(())
}