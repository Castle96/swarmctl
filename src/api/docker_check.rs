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
            let config_path = dirs().join("config.json");
            let content = std::fs::read_to_string(&config_path).ok()?;
            let config: serde_json::Value = serde_json::from_str(&content).ok()?;
            config.get("currentContext")?.as_str().map(|s| s.to_string())
        })
}

pub fn get_connection_summary(
    cli_host: &Option<String>,
    cli_context: &Option<String>,
) -> String {
    if let Some(ctx) = cli_context {
        return format!("context '{}'", ctx);
    }
    if let Some(host) = cli_host {
        return format!("host '{}'", host);
    }
    if let Some(host) = get_docker_host() {
        return format!("host '{}' (DOCKER_HOST)", host);
    }
    if let Some(ctx) = get_effective_context() {
        if ctx != "default" {
            return format!("context '{}' (from config)", ctx);
        }
    }
    "local socket".to_string()
}

fn dirs() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    std::path::PathBuf::from(home).join(".docker")
}
