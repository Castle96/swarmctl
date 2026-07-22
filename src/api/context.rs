use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct DockerConfig {
    #[serde(default)]
    pub current_context: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContextMeta {
    pub name: String,
    #[serde(default)]
    pub metadata: Option<ContextMetadata>,
    pub endpoints: HashMap<String, Endpoint>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContextMetadata {
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Endpoint {
    pub host: String,
    #[serde(default)]
    pub skip_tls_verify: Option<bool>,
    #[serde(default)]
    pub tls: Option<TlsConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TlsConfig {
    #[serde(default)]
    pub ca_file: Option<String>,
    #[serde(default)]
    pub cert_file: Option<String>,
    #[serde(default)]
    pub key_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DockerContext {
    pub name: String,
    pub description: String,
    pub host: String,
    pub tls: Option<TlsConfig>,
    pub is_current: bool,
}

fn docker_dir() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".docker")
}

fn config_path() -> PathBuf {
    docker_dir().join("config.json")
}

fn contexts_dir() -> PathBuf {
    docker_dir().join("contexts").join("meta")
}

pub fn get_current_context_name() -> Result<Option<String>> {
    let path = config_path();
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let config: DockerConfig = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(config.current_context)
}

pub fn set_current_context(name: &str) -> Result<()> {
    let path = config_path();
    let mut config: DockerConfig = if path.exists() {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?
    } else {
        DockerConfig {
            current_context: None,
        }
    };
    config.current_context = Some(name.to_string());
    let content = serde_json::to_string_pretty(&config)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub fn list_contexts() -> Result<Vec<DockerContext>> {
    let current_name = get_current_context_name()?;
    let dir = contexts_dir();
    let mut contexts = Vec::new();

    if !dir.exists() {
        return Ok(contexts);
    }

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let meta_path = entry.path().join("meta.json");
        if !meta_path.exists() {
            continue;
        }
        match read_context(&meta_path, current_name.as_deref()) {
            Ok(ctx) => contexts.push(ctx),
            Err(e) => {
                log::warn!("Skipping invalid context at {}: {}", meta_path.display(), e);
            }
        }
    }

    contexts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(contexts)
}

pub fn get_context(name: &str) -> Result<DockerContext> {
    let current_name = get_current_context_name()?;
    let dir = contexts_dir();

    if !dir.exists() {
        anyhow::bail!("No Docker contexts found");
    }

    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let meta_path = entry.path().join("meta.json");
        if !meta_path.exists() {
            continue;
        }
        let ctx = read_context(&meta_path, current_name.as_deref())?;
        if ctx.name == name {
            return Ok(ctx);
        }
    }

    anyhow::bail!("Context '{}' not found", name)
}

pub fn get_active_context() -> Result<Option<DockerContext>> {
    let current_name = get_current_context_name()?;
    match current_name {
        Some(name) => Ok(Some(get_context(&name)?)),
        None => Ok(None),
    }
}

fn read_context(meta_path: &std::path::Path, current_name: Option<&str>) -> Result<DockerContext> {
    let content = fs::read_to_string(meta_path)
        .with_context(|| format!("Failed to read {}", meta_path.display()))?;
    let meta: ContextMeta = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", meta_path.display()))?;

    let docker_endpoint = meta
        .endpoints
        .get("docker")
        .context("No 'docker' endpoint in context")?;

    let description = meta
        .metadata
        .and_then(|m| m.description)
        .unwrap_or_default();

    Ok(DockerContext {
        name: meta.name.clone(),
        description,
        host: docker_endpoint.host.clone(),
        tls: docker_endpoint.tls.clone(),
        is_current: current_name == Some(&meta.name),
    })
}

impl DockerContext {
    pub fn to_connection_config(&self) -> crate::api::client::ConnectionConfig {
        let mut config = crate::api::client::ConnectionConfig {
            host: Some(self.host.clone()),
            ..Default::default()
        };

        if let Some(tls) = &self.tls {
            config.tlscacert = tls.ca_file.clone();
            config.tlscert = tls.cert_file.clone();
            config.tlskey = tls.key_file.clone();
        }

        config
    }
}
