use crate::api::client::DockerClient;

pub async fn run(
    client: &DockerClient,
    resource: &str,
    name: String,
    filename: String,
) -> anyhow::Result<()> {
    let live_yaml = match resource {
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
        _ => anyhow::bail!("Unsupported resource type: {}", resource),
    };

    let file_yaml = std::fs::read_to_string(&filename)?;

    let mut live_file = tempfile::NamedTempFile::new()?;
    std::io::Write::write_all(&mut live_file, live_yaml.as_bytes())?;

    let mut file_file = tempfile::NamedTempFile::new()?;
    std::io::Write::write_all(&mut file_file, file_yaml.as_bytes())?;

    let output = std::process::Command::new("diff")
        .arg("-u")
        .arg("--label")
        .arg("live")
        .arg(live_file.path())
        .arg("--label")
        .arg(&filename)
        .arg(file_file.path())
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !stdout.is_empty() {
        println!("{}", stdout);
    }
    if !stderr.is_empty() {
        eprintln!("{}", stderr);
    }

    if output.status.success() {
        println!("No differences");
    }

    Ok(())
}
