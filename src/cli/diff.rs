use crate::api::client::DockerClient;

pub async fn run(
    client: &DockerClient,
    resource: &str,
    name: String,
    filename: String,
) -> anyhow::Result<()> {
    let live_yaml = match resource {
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
