use crate::api::client::DockerClient;
use anyhow::Context;
use std::io::Write;
use tempfile::NamedTempFile;

pub async fn run(client: &DockerClient, resource: &str, name: String) -> anyhow::Result<()> {
    let yaml = match resource {
        "service" | "services" => {
            let service = crate::api::service::find_service(client.inner(), &name).await?;
            serde_yaml::to_string(&service)?
        }
        "node" | "nodes" => {
            let node = crate::api::node::find_node(client.inner(), &name).await?;
            serde_yaml::to_string(&node)?
        }
        "secret" | "secrets" => {
            let secret = crate::api::secret::find_secret(client.inner(), &name).await?;
            serde_yaml::to_string(&secret)?
        }
        "config" | "configs" => {
            let config = crate::api::config::find_config(client.inner(), &name).await?;
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

    // EDITOR is often set to something like `code -w` or `emacsclient -nw`.
    // Passing that whole string to Command::new looks for a binary literally
    // named "code -w" and fails with "No such file or directory".
    let mut parts = editor.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("$EDITOR is empty"))?;
    let status = std::process::Command::new(program)
        .args(parts)
        .arg(&path)
        .status()
        .with_context(|| format!("Failed to run editor '{}'", program))?;
    if !status.success() {
        anyhow::bail!("Editor '{}' exited with {}", program, status);
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
            // Update in place. Deleting first and re-creating meant a failed
            // create lost the secret entirely.
            let existing = crate::api::secret::find_secret(client.inner(), &name).await?;
            let id = existing
                .id
                .ok_or_else(|| anyhow::anyhow!("Secret '{}' has no id", name))?;
            let version = existing.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i64;
            let opts = bollard::query_parameters::UpdateSecretOptionsBuilder::default()
                .version(version)
                .build();
            client
                .inner()
                .update_secret(&id, parsed.spec.unwrap_or_default(), opts)
                .await?;
        }
        "config" | "configs" => {
            let parsed: bollard::models::Config = serde_yaml::from_str(&edited)?;
            let existing = crate::api::config::find_config(client.inner(), &name).await?;
            let id = existing
                .id
                .ok_or_else(|| anyhow::anyhow!("Config '{}' has no id", name))?;
            let version = existing.version.as_ref().and_then(|v| v.index).unwrap_or(0) as i64;
            let opts = bollard::query_parameters::UpdateConfigOptionsBuilder::default()
                .version(version)
                .build();
            client
                .inner()
                .update_config(&id, parsed.spec.unwrap_or_default(), opts)
                .await?;
        }
        _ => unreachable!(),
    }

    println!("{} '{}' updated", resource, name);
    Ok(())
}
