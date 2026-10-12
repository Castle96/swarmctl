use crate::api::client::DockerClient;
use crate::cli::root::{OutputFormat, ResourceType};
use crate::utils::printer::{report_created, report_updated};

/// Cluster-wide options shared by every `apply` path.
///
/// Bundled rather than passed positionally: the alternative was an eight
/// argument function, two of whose arguments were `Option`s of the same type
/// and so silently swappable at the call site.
pub struct ApplyOptions<'a> {
    pub name: Option<String>,
    pub filename: Option<String>,
    pub stdin: bool,
    pub dry_run: bool,
    pub output: OutputFormat,
    /// CLI `--host`, so the drift store is keyed to the cluster actually hit.
    pub host: Option<&'a str>,
}

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    opts: ApplyOptions<'_>,
) -> anyhow::Result<()> {
    let ApplyOptions {
        name,
        filename,
        stdin,
        dry_run,
        output,
        host,
    } = opts;
    match resource {
        ResourceType::Services => {
            apply_service(client, name, filename, stdin, dry_run, output, host).await?
        }
        ResourceType::Configs => {
            apply_config(client, name, filename, stdin, dry_run, output, host).await?
        }
        ResourceType::Secrets => {
            apply_secret(client, name, filename, stdin, dry_run, output, host).await?
        }
        ResourceType::Networks => {
            apply_network(client, name, filename, stdin, dry_run, output, host).await?
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

/// The resource's name from a spec, accepting both spellings.
///
/// `get services -o json` emits the spec with `Name` (capitalised, as the
/// Docker API returns it), while a hand-written manifest may use `name`.
/// Reading only `name` silently produced the literal name "unknown", which
/// then got recorded as the drift key — so `drift` reported the resource as
/// both missing and untracked at once.
fn spec_name(spec: &serde_json::Value) -> Option<String> {
    for key in ["Name", "name"] {
        if let Some(name) = spec.get(key).and_then(|n| n.as_str())
            && !name.is_empty()
        {
            return Some(name.to_string());
        }
    }
    None
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
    host: Option<&str>,
) -> anyhow::Result<()> {
    let spec_value = read_spec(filename, stdin)?;
    let spec_json = spec_to_json(&spec_value)?;

    let service_name = name
        .clone()
        .or_else(|| spec_name(&spec_json))
        .unwrap_or_else(|| "unknown".to_string());

    let spec: bollard::models::ServiceSpec = serde_json::from_value(spec_json.clone())
        .map_err(|e| anyhow::anyhow!("Invalid service spec: {}", e))?;

    if dry_run {
        // A dry run now reports the real diff against the live service rather
        // than printing "would apply" without comparing anything. A dry run
        // that shows no differences is the difference between "this apply is a
        // no-op" and "this apply would change the cluster".
        let existing = crate::api::service::find_service_opt(client.inner(), &service_name).await?;
        let endpoint = crate::api::endpoints::current_endpoint_key_with(host);
        let stored = crate::api::applied::read(&endpoint, "services", &service_name)
            .ok()
            .flatten();

        let live_spec = match &existing {
            Some(service) => serde_json::to_value(service.spec.clone().unwrap_or_default())
                .unwrap_or(serde_json::Value::Null),
            // Not running yet: the diff is the whole manifest.
            None => serde_json::Value::Null,
        };

        // Compare against the recorded manifest when there is one, because
        // that is the declared state; otherwise against the live service.
        let desired = stored
            .map(|s| s.manifest)
            .unwrap_or_else(|| spec_json.clone());
        let changes = crate::api::specdiff::compare_objects(&live_spec, &desired);

        if existing.is_none() {
            println!("[dry run] Would create service '{}'", service_name);
        } else if changes.is_empty() {
            println!(
                "[dry run] Service '{}' already matches; no changes would be applied",
                service_name
            );
        } else {
            println!(
                "[dry run] Would update service '{}': {} field(s) would change",
                service_name,
                changes.len()
            );
            for change in &changes {
                println!(
                    "      {}: {} -> {}",
                    change.path, change.live, change.desired
                );
            }
        }
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

    // Record the manifest so `drift` has a declared state to compare against.
    // A failure here is reported but does not fail the apply: the cluster is
    // already in the requested state, and losing drift tracking is strictly
    // less bad than rolling that back.
    let endpoint = crate::api::endpoints::current_endpoint_key_with(host);
    if let Err(e) = crate::api::applied::record(&endpoint, "services", &service_name, spec_json) {
        log::warn!(
            "applied service '{}' but could not record its manifest for drift tracking: {}",
            service_name,
            e
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
    output: OutputFormat,
    _host: Option<&str>,
) -> anyhow::Result<()> {
    let spec_value = read_spec(filename, stdin)?;
    let spec_json = spec_to_json(&spec_value)?;

    let config_name = name
        .clone()
        .or_else(|| spec_name(&spec_json))
        .unwrap_or_else(|| "unknown".to_string());

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
    _host: Option<&str>,
) -> anyhow::Result<()> {
    let spec_value = read_spec(filename, stdin)?;
    let spec_json = spec_to_json(&spec_value)?;

    let secret_name = name
        .clone()
        .or_else(|| spec_name(&spec_json))
        .unwrap_or_else(|| "unknown".to_string());

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
    _host: Option<&str>,
) -> anyhow::Result<()> {
    let spec_value = read_spec(filename, stdin)?;
    let spec_json = spec_to_json(&spec_value)?;

    let network_name = name
        .clone()
        .or_else(|| spec_name(&spec_json))
        .unwrap_or_else(|| "unknown".to_string());

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `get services -o json` emits the spec with `Name` capitalised. Reading
    /// only lowercase `name` produced the literal string "unknown" as the
    /// resource name, which was then recorded as the drift key — so `drift`
    /// reported the service as both missing and untracked at once.
    #[test]
    fn spec_name_accepts_the_api_capitalised_spelling() {
        assert_eq!(spec_name(&json!({"Name": "web"})), Some("web".to_string()));
    }

    #[test]
    fn spec_name_accepts_a_lowercase_manifest() {
        assert_eq!(spec_name(&json!({"name": "web"})), Some("web".to_string()));
    }

    #[test]
    fn spec_name_prefers_the_api_spelling_when_both_exist() {
        assert_eq!(
            spec_name(&json!({"Name": "from-api", "name": "from-manifest"})),
            Some("from-api".to_string())
        );
    }

    #[test]
    fn spec_name_is_none_when_absent_or_empty() {
        assert_eq!(spec_name(&json!({})), None);
        assert_eq!(spec_name(&json!({"Name": ""})), None);
        assert_eq!(spec_name(&json!({"Name": null})), None);
    }
}
