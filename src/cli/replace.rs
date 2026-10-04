use crate::api::client::DockerClient;
use crate::cli::root::{OutputFormat, ResourceType};
use crate::utils::printer::{report_created, report_updated};
use std::io::{self, Read};

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    force: bool,
    output: OutputFormat,
) -> anyhow::Result<()> {
    match resource {
        ResourceType::Services => replace_service(client, filename, stdin, dry_run, force).await?,
        ResourceType::Configs => {
            replace_config(client, filename, stdin, dry_run, force, output).await?
        }
        ResourceType::Secrets => {
            replace_secret(client, filename, stdin, dry_run, force, output).await?
        }
        ResourceType::Networks => replace_network(client, filename, stdin, dry_run, force).await?,
        ResourceType::Nodes => replace_node(client, filename, stdin, dry_run, force).await?,
        _ => {
            return Err(anyhow::anyhow!(
                "replace is not supported for {:?}",
                resource
            ));
        }
    }

    Ok(())
}

fn read_spec_file(filename: Option<String>, stdin: bool) -> anyhow::Result<String> {
    if stdin {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        Ok(buf)
    } else if let Some(path) = filename {
        Ok(std::fs::read_to_string(&path)?)
    } else {
        Err(anyhow::anyhow!("Must specify --filename or --stdin"))
    }
}

async fn replace_service(
    client: &DockerClient,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    force: bool,
) -> anyhow::Result<()> {
    let content = read_spec_file(filename, stdin)?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .or_else(|_| serde_yaml::from_str::<serde_json::Value>(&content))?;

    let spec_value = value
        .get("spec")
        .or_else(|| value.get("ServiceSpec"))
        .unwrap_or(&value);

    let spec: bollard::models::ServiceSpec = serde_json::from_value(spec_value.clone())
        .map_err(|e| anyhow::anyhow!("Invalid service spec: {}", e))?;

    let service_name = spec.name.clone().unwrap_or_else(|| "unknown".to_string());

    if dry_run {
        println!("[dry run] Would replace service '{}'", service_name);
        return Ok(());
    }

    let services = crate::api::service::list_services(client.inner()).await?;
    let existing = services
        .into_iter()
        .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_deref()) == Some(service_name.as_str()));

    if let Some(existing) = existing {
        let version = existing.version.and_then(|v| v.index).unwrap_or(0) as i32;

        if force {
            client.inner().delete_service(&service_name).await?;
            let result = client.inner().create_service(spec, None).await?;
            println!(
                "Service '{}' replaced (ID: {})",
                service_name,
                result.id.unwrap_or_default()
            );
        } else {
            let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
                .version(version)
                .build();
            client
                .inner()
                .update_service(&service_name, spec, opts, None)
                .await?;
            println!("Service '{}' replaced", service_name);
        }
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

async fn replace_config(
    client: &DockerClient,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    force: bool,
    output: OutputFormat,
) -> anyhow::Result<()> {
    let content = read_spec_file(filename, stdin)?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .or_else(|_| serde_yaml::from_str::<serde_json::Value>(&content))?;

    let spec_value = value
        .get("spec")
        .or_else(|| value.get("ConfigSpec"))
        .unwrap_or(&value);

    let spec: bollard::models::ConfigSpec = serde_json::from_value(spec_value.clone())
        .map_err(|e| anyhow::anyhow!("Invalid config spec: {}", e))?;

    let config_name = spec.name.clone().unwrap_or_else(|| "unknown".to_string());

    if dry_run {
        println!("[dry run] Would replace config '{}'", config_name);
        return Ok(());
    }

    match crate::api::config::find_config_opt(client.inner(), &config_name).await? {
        Some(existing) => {
            if !force {
                // Previously this printed a hint and exited 0, so a script
                // believed it had replaced the config when nothing changed.
                anyhow::bail!(
                    "Config '{}' already exists. Re-run with --force to replace it, or use \
                     `swarmctl edit config {}`.",
                    config_name,
                    config_name
                );
            }
            // Update in place. Deleting first and re-creating meant a failed
            // create left the config gone.
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
        }
        None => {
            let result = client.inner().create_config(spec).await?;
            report_created("Config", &config_name, &result, &output)?;
        }
    }

    Ok(())
}

async fn replace_secret(
    client: &DockerClient,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    force: bool,
    output: OutputFormat,
) -> anyhow::Result<()> {
    let content = read_spec_file(filename, stdin)?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .or_else(|_| serde_yaml::from_str::<serde_json::Value>(&content))?;

    let spec_value = value
        .get("spec")
        .or_else(|| value.get("SecretSpec"))
        .unwrap_or(&value);

    let spec: bollard::models::SecretSpec = serde_json::from_value(spec_value.clone())
        .map_err(|e| anyhow::anyhow!("Invalid secret spec: {}", e))?;

    let secret_name = spec.name.clone().unwrap_or_else(|| "unknown".to_string());

    if dry_run {
        println!("[dry run] Would replace secret '{}'", secret_name);
        return Ok(());
    }

    match crate::api::secret::find_secret_opt(client.inner(), &secret_name).await? {
        Some(existing) => {
            if !force {
                anyhow::bail!(
                    "Secret '{}' already exists. Re-run with --force to replace it, or use \
                     `swarmctl edit secret {}`.",
                    secret_name,
                    secret_name
                );
            }
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
        }
        None => {
            let result = client.inner().create_secret(spec).await?;
            report_created("Secret", &secret_name, &result, &output)?;
        }
    }

    Ok(())
}

async fn replace_network(
    client: &DockerClient,
    filename: Option<String>,
    stdin: bool,
    dry_run: bool,
    force: bool,
) -> anyhow::Result<()> {
    let content = read_spec_file(filename, stdin)?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .or_else(|_| serde_yaml::from_str::<serde_json::Value>(&content))?;

    let spec_value = value
        .get("spec")
        .or_else(|| value.get("NetworkSpec"))
        .unwrap_or(&value);

    let spec: bollard::models::NetworkCreateRequest = serde_json::from_value(spec_value.clone())
        .map_err(|e| anyhow::anyhow!("Invalid network spec: {}", e))?;

    let network_name = spec.name.clone();

    if dry_run {
        println!("[dry run] Would replace network '{}'", network_name);
        return Ok(());
    }

    let networks = crate::api::network::list_networks(client.inner()).await?;
    let existing = networks
        .into_iter()
        .find(|n| n.name.as_deref() == Some(&network_name));

    if let Some(existing) = existing {
        if force {
            let id = existing.id.as_deref().unwrap_or(&network_name);
            client.inner().remove_network(id).await?;
            client.inner().create_network(spec).await?;
            println!("Network '{}' replaced", network_name);
        } else {
            println!(
                "Network '{}' already exists (use --force to replace)",
                network_name
            );
        }
    } else {
        client.inner().create_network(spec).await?;
        println!("Network '{}' created", network_name);
    }

    Ok(())
}

async fn replace_node(
    _client: &DockerClient,
    _filename: Option<String>,
    _stdin: bool,
    _dry_run: bool,
    _force: bool,
) -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "replace is not supported for nodes. Use 'swarmctl patch' or 'swarmctl label' instead."
    ))
}
