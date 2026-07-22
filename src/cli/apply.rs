use crate::api::client::DockerClient;
use crate::cli::root::ResourceType;

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    match resource {
        ResourceType::Services => apply_service(client, name, filename, stdin, dry_run).await?,
        ResourceType::Configs => apply_config(client, name, filename, stdin, dry_run).await?,
        ResourceType::Secrets => apply_secret(client, name, filename, stdin, dry_run).await?,
        ResourceType::Networks => apply_network(client, name, filename, stdin, dry_run).await?,
        _ => {
            return Err(anyhow::anyhow!(
                "apply is not yet supported for {:?}",
                resource
            ));
        }
    }

    Ok(())
}

fn read_spec(filename: Option<String>, stdin: bool) -> anyhow::Result<serde_json::Value> {
    let content = if stdin {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else if let Some(path) = filename {
        std::fs::read_to_string(&path)?
    } else {
        return Err(anyhow::anyhow!("Must specify --filename or --stdin"));
    };

    let value: serde_json::Value = serde_json::from_str(&content).or_else(|_| {
        serde_yaml::from_str::<serde_json::Value>(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse as JSON or YAML: {}", e))
    })?;

    Ok(value)
}

fn spec_to_json(value: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let spec = value
        .get("spec")
        .or_else(|| value.get("ServiceSpec"))
        .or_else(|| value.get("ConfigSpec"))
        .or_else(|| value.get("SecretSpec"))
        .or_else(|| value.get("NetworkSpec"))
        .unwrap_or(value);
    Ok(spec.clone())
}

async fn apply_service(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    let spec_value = read_spec(filename, stdin)?;
    let spec_json = spec_to_json(&spec_value)?;

    let service_name = name.clone().unwrap_or_else(|| {
        spec_json
            .get("name")
            .and_then(|n| n.as_str().map(String::from))
            .unwrap_or_else(|| "unknown".to_string())
    });

    let spec: bollard::models::ServiceSpec = serde_json::from_value(spec_json)
        .map_err(|e| anyhow::anyhow!("Invalid service spec: {}", e))?;

    if dry_run {
        println!("[dry run] Would apply service '{}'", service_name);
        return Ok(());
    }

    let services = crate::api::service::list_services(client.inner()).await?;
    let existing = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&service_name));

    if let Some(existing) = existing {
        let version = existing.version.and_then(|v| v.index).unwrap_or(0) as i32;
        let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
            .version(version)
            .build();
        client
            .inner()
            .update_service(&service_name, spec, opts, None)
            .await?;
        println!("Service '{}' updated", service_name);
    } else {
        let result = client.inner().create_service(spec, None).await?;
        println!(
            "Service '{}' created (ID: {})",
            service_name,
            result.id.unwrap_or_default()
        );
    }

    Ok(())
}

async fn apply_config(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    let spec_value = read_spec(filename, stdin)?;
    let spec_json = spec_to_json(&spec_value)?;

    let config_name = name.clone().unwrap_or_else(|| {
        spec_json
            .get("name")
            .and_then(|n| n.as_str().map(String::from))
            .unwrap_or_else(|| "unknown".to_string())
    });

    let spec: bollard::models::ConfigSpec = serde_json::from_value(spec_json)
        .map_err(|e| anyhow::anyhow!("Invalid config spec: {}", e))?;

    if dry_run {
        println!("[dry run] Would apply config '{}'", config_name);
        return Ok(());
    }

    let configs = crate::api::config::list_configs(client.inner()).await?;
    let existing = configs
        .into_iter()
        .find(|c| c.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&config_name));

    if existing.is_some() {
        anyhow::bail!(
            "Config '{}' already exists; delete first to recreate",
            config_name
        );
    } else {
        client.inner().create_config(spec).await?;
        println!("Config '{}' created", config_name);
    }

    Ok(())
}

async fn apply_secret(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    let spec_value = read_spec(filename, stdin)?;
    let spec_json = spec_to_json(&spec_value)?;

    let secret_name = name.clone().unwrap_or_else(|| {
        spec_json
            .get("name")
            .and_then(|n| n.as_str().map(String::from))
            .unwrap_or_else(|| "unknown".to_string())
    });

    let spec: bollard::models::SecretSpec = serde_json::from_value(spec_json)
        .map_err(|e| anyhow::anyhow!("Invalid secret spec: {}", e))?;

    if dry_run {
        println!("[dry run] Would apply secret '{}'", secret_name);
        return Ok(());
    }

    let secrets = crate::api::secret::list_secrets(client.inner()).await?;
    let existing = secrets
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&secret_name));

    if existing.is_some() {
        anyhow::bail!(
            "Secret '{}' already exists; delete first to recreate",
            secret_name
        );
    } else {
        client.inner().create_secret(spec).await?;
        println!("Secret '{}' created", secret_name);
    }

    Ok(())
}

async fn apply_network(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    let spec_value = read_spec(filename, stdin)?;
    let spec_json = spec_to_json(&spec_value)?;

    let network_name = name.clone().unwrap_or_else(|| {
        spec_json
            .get("name")
            .and_then(|n| n.as_str().map(String::from))
            .unwrap_or_else(|| "unknown".to_string())
    });

    let spec: bollard::models::NetworkCreateRequest = serde_json::from_value(spec_json)
        .map_err(|e| anyhow::anyhow!("Invalid network spec: {}", e))?;

    if dry_run {
        println!("[dry run] Would apply network '{}'", network_name);
        return Ok(());
    }

    let networks = crate::api::network::list_networks(client.inner()).await?;
    let existing = networks
        .into_iter()
        .find(|n| n.name.as_deref() == Some(&network_name));

    if existing.is_some() {
        println!("Network '{}' already exists, skipping", network_name);
    } else {
        client.inner().create_network(spec).await?;
        println!("Network '{}' created", network_name);
    }

    Ok(())
}
