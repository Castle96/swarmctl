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

    let differs = !output.status.success();

    if !stdout.is_empty() {
        print!("{}", stdout);
    }
    if !stderr.is_empty() {
        eprintln!("{}", stderr);
    }

    // `diff` is only useful in a script if it follows the same convention as the
    // system tool: 0 when the inputs match, 1 when they differ. It previously
    // always exited 0, so `diff ... && deploy` passed on a mismatch.
    if !differs {
        println!("No differences");
        return Ok(());
    }

    // A diff exit code of 2 means the tool itself failed (missing binary, bad
    // arguments), which should not masquerade as "files differ".
    if output.status.code() == Some(2) {
        anyhow::bail!("`diff` exited with an error (code 2). Is the diff tool installed?");
    }

    std::process::exit(1);
}
