use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Docker's `config.json`.
///
/// Only the keys swarmctl cares about are modelled. Everything else in the
/// file (`auths`, `credsStore`, `psFormat`, `experimental`, ...) is preserved
/// verbatim on write by operating on a `serde_json::Value`; see
/// [`set_current_context_in`].
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DockerConfig {
    #[serde(rename = "currentContext", default)]
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

/// A context endpoint as Docker writes it.
///
/// `TLS` is not the PascalCase of `tls`, so the rename is spelled out. The same
/// applies to the certificate fields below.
#[derive(Debug, Clone, Deserialize)]
pub struct Endpoint {
    #[serde(rename = "Host")]
    pub host: String,
    #[serde(rename = "SkipTLSVerify", default)]
    pub skip_tls_verify: Option<bool>,
    #[serde(rename = "TLS", default)]
    pub tls: Option<TlsConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct TlsConfig {
    #[serde(rename = "CAFile", default)]
    pub ca_file: Option<String>,
    #[serde(rename = "CertFile", default)]
    pub cert_file: Option<String>,
    #[serde(rename = "KeyFile", default)]
    pub key_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DockerContext {
    pub name: String,
    pub description: String,
    pub host: String,
    pub tls: Option<TlsConfig>,
    pub skip_tls_verify: bool,
    pub is_current: bool,
}

fn docker_dir() -> PathBuf {
    crate::api::docker_check::docker_config_dir().unwrap_or_else(|| PathBuf::from(".docker"))
}

/// Docker config root. Threaded explicitly rather than read from the environment
/// so the context logic is testable without mutating process-global state.
type DockerDir = PathBuf;

fn config_path_in(dir: &DockerDir) -> PathBuf {
    dir.join("config.json")
}

fn contexts_dir_in(dir: &DockerDir) -> PathBuf {
    dir.join("contexts").join("meta")
}

pub fn get_current_context_name() -> Result<Option<String>> {
    get_current_context_name_in(&docker_dir())
}

pub fn get_current_context_name_in(dir: &DockerDir) -> Result<Option<String>> {
    let path = config_path_in(dir);
    if !path.exists() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let config: DockerConfig = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(config.current_context)
}

/// Switch Docker's active context without clobbering the rest of `config.json`.
///
/// Docker's config file also holds registry credentials (`auths`),
/// `credsStore`, and assorted user preferences. Deserializing it into a typed
/// struct and serializing it back would drop every field we don't model, so
/// this edits the raw JSON in place and touches only `currentContext`.
pub fn set_current_context(name: &str) -> Result<()> {
    set_current_context_in(&docker_dir(), name)
}

pub fn set_current_context_in(dir: &DockerDir, name: &str) -> Result<()> {
    let path = config_path_in(dir);

    let mut config: serde_json::Value = if path.exists() {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        // An empty or non-object file is treated as empty rather than fatal, so a
        // corrupt config can still be repaired by switching contexts.
        match serde_json::from_str(&content) {
            Ok(v @ serde_json::Value::Object(_)) => v,
            Ok(_) => serde_json::Value::Object(Default::default()),
            Err(e) => {
                log::warn!(
                    "{} is not valid JSON ({}); rewriting it with only currentContext",
                    path.display(),
                    e
                );
                serde_json::Value::Object(Default::default())
            }
        }
    } else {
        serde_json::Value::Object(Default::default())
    };

    config["currentContext"] = serde_json::Value::String(name.to_string());

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(&config)?;
    fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub fn list_contexts() -> Result<Vec<DockerContext>> {
    list_contexts_in(&docker_dir())
}

pub fn list_contexts_in(dir: &DockerDir) -> Result<Vec<DockerContext>> {
    let current_name = get_current_context_name_in(dir)?;
    let contexts_path = contexts_dir_in(dir);
    let mut contexts = Vec::new();

    if !contexts_path.exists() {
        return Ok(contexts);
    }

    for entry in fs::read_dir(&contexts_path)? {
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
    get_context_in(&docker_dir(), name)
}

pub fn get_context_in(dir: &DockerDir, name: &str) -> Result<DockerContext> {
    let current_name = get_current_context_name_in(dir)?;
    let contexts_path = contexts_dir_in(dir);

    if !contexts_path.exists() {
        anyhow::bail!("No Docker contexts found");
    }

    for entry in fs::read_dir(&contexts_path)? {
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
    get_active_context_in(&docker_dir())
}

pub fn get_active_context_in(dir: &DockerDir) -> Result<Option<DockerContext>> {
    match get_current_context_name_in(dir)? {
        Some(name) => Ok(Some(get_context_in(dir, &name)?)),
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
        skip_tls_verify: docker_endpoint.skip_tls_verify.unwrap_or(false),
        is_current: current_name == Some(&meta.name),
    })
}

impl DockerContext {
    pub fn to_connection_config(&self) -> crate::api::client::ConnectionConfig {
        let mut config = crate::api::client::ConnectionConfig {
            host: Some(self.host.clone()),
            ..Default::default()
        };

        config.skip_tls_verify = self.skip_tls_verify;

        if let Some(tls) = &self.tls {
            config.tlscacert = tls.ca_file.clone();
            config.tlscert = tls.cert_file.clone();
            config.tlskey = tls.key_file.clone();
        }

        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip through a throwaway Docker config directory.
    #[test]
    fn set_current_context_preserves_unrelated_config_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            config_path_in(&dir),
            r#"{
                "auths": {"registry.example.com": {"auth": "c2VjcmV0"}},
                "credsStore": "secretservice",
                "psFormat": "table {{.ID}}",
                "experimental": "enabled",
                "currentContext": "old"
            }"#,
        )
        .unwrap();

        set_current_context_in(&dir, "prod").unwrap();

        let raw = std::fs::read_to_string(config_path_in(&dir)).unwrap();
        let after: serde_json::Value = serde_json::from_str(&raw).unwrap();

        assert_eq!(after["currentContext"], serde_json::json!("prod"));
        assert_eq!(
            after["auths"]["registry.example.com"]["auth"],
            serde_json::json!("c2VjcmV0"),
            "registry credentials must survive a context switch"
        );
        assert_eq!(after["credsStore"], serde_json::json!("secretservice"));
        assert_eq!(after["psFormat"], serde_json::json!("table {{.ID}}"));
        assert_eq!(after["experimental"], serde_json::json!("enabled"));
    }

    #[test]
    fn set_current_context_uses_dockers_camel_case_key() {
        // Docker reads `currentContext`; the previous PascalCase rename emitted
        // `CurrentContext`, which Docker silently ignored.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        std::fs::write(config_path_in(&dir), "{}").unwrap();
        set_current_context_in(&dir, "prod").unwrap();
        let raw = std::fs::read_to_string(config_path_in(&dir)).unwrap();
        assert!(raw.contains("currentContext"), "got: {raw}");
        assert!(!raw.contains("CurrentContext"), "got: {raw}");
    }

    #[test]
    fn set_current_context_creates_config_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        set_current_context_in(&dir, "prod").unwrap();
        let raw = std::fs::read_to_string(config_path_in(&dir)).unwrap();
        let after: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(after["currentContext"], serde_json::json!("prod"));
    }

    #[test]
    fn set_current_context_recovers_from_corrupt_config() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        std::fs::write(config_path_in(&dir), "{not json").unwrap();
        // Must not hard-fail: a corrupt config can still be repaired by
        // switching contexts.
        set_current_context_in(&dir, "prod").unwrap();
        let raw = std::fs::read_to_string(config_path_in(&dir)).unwrap();
        let after: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(after["currentContext"], serde_json::json!("prod"));
    }

    #[test]
    fn current_context_round_trips_through_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        assert_eq!(get_current_context_name_in(&dir).unwrap(), None);
        set_current_context_in(&dir, "prod").unwrap();
        assert_eq!(
            get_current_context_name_in(&dir).unwrap().as_deref(),
            Some("prod")
        );
    }

    #[test]
    fn reads_docker_style_tls_field_names() {
        // Docker writes TLS/CAFile/CertFile/KeyFile; a blanket PascalCase rename
        // looks for Tls/CaFile/... and silently loses the configuration.
        let meta: ContextMeta = serde_json::from_str(
            r#"{
                "Name": "prod",
                "Metadata": {"Description": "d"},
                "Endpoints": {"docker": {
                    "Host": "tcp://10.0.0.1:2376",
                    "SkipTLSVerify": false,
                    "TLS": {
                        "CAFile": "/ca.pem",
                        "CertFile": "/cert.pem",
                        "KeyFile": "/key.pem"
                    }
                }}
            }"#,
        )
        .unwrap();

        let ep = meta.endpoints.get("docker").unwrap();
        let tls = ep.tls.as_ref().expect("TLS block must be parsed");
        assert_eq!(tls.ca_file.as_deref(), Some("/ca.pem"));
        assert_eq!(tls.cert_file.as_deref(), Some("/cert.pem"));
        assert_eq!(tls.key_file.as_deref(), Some("/key.pem"));
        assert_eq!(ep.skip_tls_verify, Some(false));

        let ctx = DockerContext {
            name: meta.name.clone(),
            description: meta.metadata.unwrap().description.unwrap_or_default(),
            host: ep.host.clone(),
            tls: ep.tls.clone(),
            skip_tls_verify: ep.skip_tls_verify.unwrap_or(false),
            is_current: false,
        };
        let cfg = ctx.to_connection_config();
        assert_eq!(cfg.tlscacert.as_deref(), Some("/ca.pem"));
        assert_eq!(cfg.tlskey.as_deref(), Some("/key.pem"));
        assert!(!cfg.skip_tls_verify);
    }

    #[test]
    fn context_without_tls_block_parses() {
        let meta: ContextMeta =
            serde_json::from_str(r#"{"Name":"p","Endpoints":{"docker":{"Host":"ssh://u@h"}}}"#)
                .unwrap();
        let ep = meta.endpoints.get("docker").unwrap();
        assert!(ep.tls.is_none());
        assert_eq!(ep.host, "ssh://u@h");
    }

    #[test]
    fn list_contexts_reads_docker_layout_and_marks_current() {
        // Docker names the context directory sha256(name), not the bare name.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let digest = "9d2b1f4c1a";
        let ctx_dir = contexts_dir_in(&dir).join(digest);
        std::fs::create_dir_all(&ctx_dir).unwrap();
        std::fs::write(
            ctx_dir.join("meta.json"),
            r#"{"Name":"prod","Metadata":{"Description":"prod cluster"},
                "Endpoints":{"docker":{"Host":"tcp://10.0.0.1:2376","SkipTLSVerify":false}}}"#,
        )
        .unwrap();
        set_current_context_in(&dir, "prod").unwrap();

        let contexts = list_contexts_in(&dir).unwrap();
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].name, "prod");
        assert_eq!(contexts[0].host, "tcp://10.0.0.1:2376");
        assert_eq!(contexts[0].description, "prod cluster");
        assert!(contexts[0].is_current);

        let fetched = get_context_in(&dir, "prod").unwrap();
        assert_eq!(fetched.name, "prod");
        assert!(get_context_in(&dir, "missing").is_err());
    }

    #[test]
    fn list_contexts_skips_corrupt_entries_instead_of_failing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let good = contexts_dir_in(&dir).join("good");
        let bad = contexts_dir_in(&dir).join("bad");
        std::fs::create_dir_all(&good).unwrap();
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(
            good.join("meta.json"),
            r#"{"Name":"ok","Endpoints":{"docker":{"Host":"ssh://h"}}}"#,
        )
        .unwrap();
        std::fs::write(bad.join("meta.json"), "{ broken").unwrap();

        let contexts = list_contexts_in(&dir).unwrap();
        assert_eq!(
            contexts.len(),
            1,
            "one bad context must not hide the good one"
        );
        assert_eq!(contexts[0].name, "ok");
    }
}
