use crate::api::client::DockerClient;
use std::io::Write;
use tempfile::NamedTempFile;

pub async fn run(client: &DockerClient, resource: &str, name: String) -> anyhow::Result<()> {
    let yaml = match resource {
        "service" | "services" => {
            let services = crate::api::service::list_services(client.inner()).await?;
            let service = services
                .into_iter()
                .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name))
                .ok_or_else(|| anyhow::anyhow!("Service '{}' not found", name))?;
            serde_yaml::to_string(&service)?
        }
        "node" | "nodes" => {
            let nodes = crate::api::node::list_nodes(client.inner()).await?;
            let node = nodes
                .into_iter()
                .find(|n| n.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name))
                .ok_or_else(|| anyhow::anyhow!("Node '{}' not found", name))?;
            serde_yaml::to_string(&node)?
        }
        "secret" | "secrets" => {
            let secrets = crate::api::secret::list_secrets(client.inner()).await?;
            let secret = secrets
                .into_iter()
                .find(|s| s.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name))
                .ok_or_else(|| anyhow::anyhow!("Secret '{}' not found", name))?;
            serde_yaml::to_string(&secret)?
        }
        "config" | "configs" => {
            let configs = crate::api::config::list_configs(client.inner()).await?;
            let config = configs
                .into_iter()
                .find(|c| c.spec.as_ref().and_then(|sp| sp.name.as_ref()) == Some(&name))
                .ok_or_else(|| anyhow::anyhow!("Config '{}' not found", name))?;
            serde_yaml::to_string(&config)?
        }
        _ => anyhow::bail!("Unsupported resource type for edit: {}", resource),
    };

    let mut tmp = NamedTempFile::new()?;
    tmp.write_all(yaml.as_bytes())?;
    let path = tmp.path().to_str().unwrap_or("").to_string();

    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".to_string());

    let status = std::process::Command::new(&editor).arg(&path).status()?;
    if !status.success() {
        anyhow::bail!("Editor exited with error");
    }

    let edited = std::fs::read_to_string(&path)?;
    let edited = edited.trim().to_string();
    if edited.is_empty() || edited == yaml.trim() {
        println!("No changes made");
        return Ok(());
    }

    match resource {
        "service" | "services" => {
            let parsed: bollard::models::Service = serde_yaml::from_str(&edited)?;
            let version = parsed.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i32;
            let spec = parsed.spec.unwrap_or_default();
            let opts = bollard::query_parameters::UpdateServiceOptionsBuilder::default()
                .version(version)
                .build();
            client
                .inner()
                .update_service(&name, spec, opts, None)
                .await?;
        }
        "node" | "nodes" => {
            let parsed: bollard::models::Node = serde_yaml::from_str(&edited)?;
            let version = parsed.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i64;
            let spec = parsed.spec.unwrap_or_default();
            let opts = bollard::query_parameters::UpdateNodeOptionsBuilder::default()
                .version(version)
                .build();
            client.inner().update_node(&name, spec, opts).await?;
        }
        "secret" | "secrets" => {
            let parsed: bollard::models::Secret = serde_yaml::from_str(&edited)?;
            client.inner().delete_secret(&name).await?;
            let spec = parsed.spec.unwrap_or_default();
            client.inner().create_secret(spec).await?;
        }
        "config" | "configs" => {
            let parsed: bollard::models::Config = serde_yaml::from_str(&edited)?;
            client.inner().delete_config(&name).await?;
            let spec = parsed.spec.unwrap_or_default();
            client.inner().create_config(spec).await?;
        }
        _ => unreachable!(),
    }

    println!("{} '{}' updated", resource, name);
    Ok(())
}
