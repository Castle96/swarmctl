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
        ResourceType::Services => {
            replace_service(client, filename, stdin, dry_run, force, output).await?
        }
        ResourceType::Configs => {
            replace_config(client, filename, stdin, dry_run, force, output).await?
        }
        ResourceType::Secrets => {
            replace_secret(client, filename, stdin, dry_run, force, output).await?
        }
        ResourceType::Networks => {
            replace_network(client, filename, stdin, dry_run, force, output).await?
        }
        // Returns an explanatory error naming the commands that do work.
        // Reached from the `Nodes` arm, so it rejects rather than reporting a
        // successful replace.
        ResourceType::Nodes => replace_node()?,
        _ => {
            return Err(anyhow::anyhow!("replace is not supported for {}", resource));
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
    output: OutputFormat,
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
            // Honours `-o json|yaml`; previously `--output` was dropped for
            // services, so `replace services -f x.json -o json` printed prose.
            report_created("Service", &service_name, &result, &output)?;
        } else {
            let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
                .version(version)
                .build();
            let updated = client
                .inner()
                .update_service(&service_name, spec, opts, None)
                .await?;
            report_updated("Service", &service_name, &updated, &output)?;
        }
    } else {
        let result = client.inner().create_service(spec, None).await?;
        report_created("Service", &service_name, &result, &output)?;
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
    output: OutputFormat,
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
            let created = client.inner().create_network(spec).await?;
            report_created("Network", &network_name, &created, &output)?;
        } else {
            // Not an error: the network already exists. Report it in the
            // requested format rather than always as prose.
            report_updated(
                "Network",
                &network_name,
                &serde_json::json!({
                    "Id": existing.id,
                    "Name": network_name,
                    "replaced": false,
                    "hint": "already exists; pass --force to replace",
                }),
                &output,
            )?;
        }
    } else {
        let created = client.inner().create_network(spec).await?;
        report_created("Network", &network_name, &created, &output)?;
    }

    Ok(())
}

/// Nodes have no replace endpoint: the Engine API exposes update (patch) and
/// label changes, but no way to swap a node's whole spec.
///
/// Takes no client, because it never dials the daemon — rejecting locally is
/// the whole point, and it makes the behaviour testable.
fn replace_node() -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "replace is not supported for nodes: the Docker API has no node replace \
         endpoint. Use `swarmctl patch nodes <name> -p '<json>'` or \
         `swarmctl label nodes <name> k=v`."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::root::OutputFormat;

    /// Every kind that reports a `replace` result routes through these helpers,
    /// so `replace services -f x.json -o json` prints JSON like
    /// `replace secrets` does instead of prose. Before the fix, services,
    /// networks and nodes dropped `--output` entirely.
    #[test]
    fn report_helpers_accept_every_output_format() {
        let payload = serde_json::json!({"Id": "abc"});
        for output in [
            OutputFormat::Table,
            OutputFormat::Wide,
            OutputFormat::Json,
            OutputFormat::Yaml,
            OutputFormat::Name,
        ] {
            assert!(report_created("Service", "web", &payload, &output).is_ok());
            assert!(report_updated("Service", "web", &payload, &output).is_ok());
        }
    }

    /// Nodes have no replace endpoint. The error must say which commands do
    /// work rather than a bare "unsupported".
    #[test]
    fn node_replace_error_points_at_the_working_commands() {
        let err = replace_node().expect_err("nodes cannot be replaced");
        let msg = err.to_string();
        assert!(msg.contains("patch"), "should name `patch`: {msg}");
        assert!(msg.contains("label"), "should name `label`: {msg}");
    }
}
