use std::process::Command;

pub fn is_docker_installed() -> bool {
    Command::new("docker")
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn get_docker_host() -> Option<String> {
    std::env::var("DOCKER_HOST").ok().filter(|s| !s.is_empty())
}

pub fn get_effective_context() -> Option<String> {
    std::env::var("DOCKER_CONTEXT")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let config_path = docker_config_dir()?.join("config.json");
            let content = std::fs::read_to_string(&config_path).ok()?;
            let config: serde_json::Value = serde_json::from_str(&content).ok()?;
            config
                .get("currentContext")?
                .as_str()
                .map(|s| s.to_string())
        })
}

pub fn get_connection_summary(cli_host: &Option<String>, cli_context: &Option<String>) -> String {
    if let Some(ctx) = cli_context {
        return format!("context '{}'", ctx);
    }
    if let Some(host) = cli_host {
        return format!("host '{}'", host);
    }
    if let Some(host) = get_docker_host() {
        return format!("host '{}' (DOCKER_HOST)", host);
    }
    if let Some(ctx) = get_effective_context()
        && ctx != "default"
    {
        return format!("context '{}' (from config)", ctx);
    }
    "local socket".to_string()
}

/// Location of the Docker CLI config directory (`~/.docker`).
///
/// `directories::BaseDirs` resolves the home directory per platform
/// (`%USERPROFILE%` on Windows, `$HOME` on Unix). Docker keeps its CLI config
/// in `~/.docker` on every platform.
pub fn docker_config_dir() -> Option<std::path::PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().join(".docker"))
}
