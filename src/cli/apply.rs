use crate::api::client::DockerClient;
use crate::cli::root::{OutputFormat, ResourceType};
use crate::utils::printer::{report_created, report_updated};

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
            apply_service(client, name, filename, stdin, dry_run, output).await?
        }
        ResourceType::Configs => {
            apply_config(client, name, filename, stdin, dry_run, output).await?
        }
        ResourceType::Secrets => {
            apply_secret(client, name, filename, stdin, dry_run, output).await?
        }
        ResourceType::Networks => {
            apply_network(client, name, filename, stdin, dry_run, output).await?
        }
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
    output: OutputFormat,
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

    let existing = crate::api::service::find_service_opt(client.inner(), &service_name).await?;

    if let Some(existing) = existing {
        let version = existing.version.and_then(|v| v.index).unwrap_or(0) as i32;
        let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
            .version(version)
            .build();
        // Re-inspect so the reported object reflects the update, not the request.
        client
            .inner()
            .update_service(&service_name, spec, opts, None)
            .await?;
        let updated = client
            .inner()
            .inspect_service(&service_name, None)
            .await
            .ok();
        match updated {
            Some(service) => report_updated("Service", &service_name, &service, &output)?,
            None => println!("Service '{}' updated", service_name),
        }
    } else {
        let result = client.inner().create_service(spec, None).await?;
        report_created("Service", &service_name, &result, &output)?;
    }

    Ok(())
}

async fn apply_config(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    output: OutputFormat,
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

    let existing = crate::api::config::find_config_opt(client.inner(), &config_name).await?;

    if existing.is_some() {
        anyhow::bail!(
            "Config '{}' already exists; delete first to recreate",
            config_name
        );
    } else {
        let result = client.inner().create_config(spec).await?;
        report_created("Config", &config_name, &result, &output)?;
    }

    Ok(())
}

async fn apply_secret(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    output: OutputFormat,
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

    let existing = crate::api::secret::find_secret_opt(client.inner(), &secret_name).await?;

    if existing.is_some() {
        anyhow::bail!(
            "Secret '{}' already exists; delete first to recreate",
            secret_name
        );
    } else {
        let result = client.inner().create_secret(spec).await?;
        report_created("Secret", &secret_name, &result, &output)?;
    }

    Ok(())
}

async fn apply_network(
    client: &DockerClient,
    name: Option<String>,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    output: OutputFormat,
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
        let result = client.inner().create_network(spec).await?;
        report_created("Network", &network_name, &result, &output)?;
    }

    Ok(())
}
