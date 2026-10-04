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

    // apply means "make it match the file". Configs and secrets support an
    // in-place update, so an existing object is updated rather than refused —
    // this used to error out with "delete first to recreate", which is not what
    // apply is for.
    if let Some(existing) =
        crate::api::config::find_config_opt(client.inner(), &config_name).await?
    {
        let id = existing
            .id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Config '{}' has no id", config_name))?;
        let version = existing.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i64;
        let opts = bollard::query_parameters::UpdateConfigOptionsBuilder::default()
            .version(version)
            .build();
        client.inner().update_config(&id, spec, opts).await?;
        let updated = client
            .inner()
            .inspect_config(&config_name)
            .await
            .unwrap_or(existing);
        report_updated("Config", &config_name, &updated, &output)?;
        return Ok(());
    }
    {
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

    if let Some(existing) =
        crate::api::secret::find_secret_opt(client.inner(), &secret_name).await?
    {
        let id = existing
            .id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Secret '{}' has no id", secret_name))?;
        let version = existing.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i64;
        let opts = bollard::query_parameters::UpdateSecretOptionsBuilder::default()
            .version(version)
            .build();
        client.inner().update_secret(&id, spec, opts).await?;
        let updated = client
            .inner()
            .inspect_secret(&secret_name)
            .await
            .unwrap_or(existing);
        report_updated("Secret", &secret_name, &updated, &output)?;
        return Ok(());
    }
    {
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

    // The Docker API has no way to update a network's spec in place, so an
    // existing one is left alone. Say so explicitly rather than reporting
    // success as though the file had been applied.
    if let Some(existing) = existing {
        let name = existing
            .name
            .clone()
            .unwrap_or_else(|| network_name.clone());
        println!(
            "Network '{}' already exists and networks cannot be updated in place; left \
             unchanged. Delete it first (and recreate its containers) if you need to \
             change it.",
            name
        );
        eprintln!(
            "warning: network '{}' was not modified; `apply` cannot update a network",
            name
        );
    } else {
        let result = client.inner().create_network(spec).await?;
        report_created("Network", &network_name, &result, &output)?;
    }

    Ok(())
}
