//! Endpoint registry: named remote/offsite Docker daemons.
//!
//! Docker contexts can address a remote daemon, but carry no notion of which
//! site or environment they belong to, so a homelab and two production regions
//! are indistinguishable. This module keeps its own registry with that metadata
//! and, in addition, materialises Docker-compatible context directories so
//! `docker --context <name>` keeps working.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// TLS material for an endpoint. Shared with the Docker context layer so there
/// is a single representation of "a certificate triple".
pub use crate::api::context::TlsConfig;

/// Bumped when the on-disk shape changes incompatibly.
pub const REGISTRY_VERSION: u32 = 1;

/// A named Docker endpoint, usually a remote manager node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub name: String,

    /// `tcp://`, `ssh://` or `unix://` address.
    pub host: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Logical site, e.g. `eu-west`. Grouping only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,

    /// `prod`, `staging`, `dev`, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<TlsConfig>,

    /// Whether this endpoint is the default for fan-out when none is named.
    #[serde(default)]
    pub default: bool,
}

/// On-disk registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry {
    pub version: u32,
    #[serde(default)]
    pub endpoints: BTreeMap<String, Endpoint>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            endpoints: BTreeMap::new(),
        }
    }
}

impl Registry {
    /// Load the registry, returning an empty one if the file does not exist.
    pub fn load() -> Result<Self> {
        Self::load_from(&crate::utils::paths::endpoints_file())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Registry::default());
        }
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        match serde_json::from_str::<Registry>(&raw) {
            Ok(reg) if reg.version > REGISTRY_VERSION => {
                anyhow::bail!(
                    "{} was written by a newer swarmctl (registry version {} > {}). \
                     Upgrade swarmctl to read it.",
                    path.display(),
                    reg.version,
                    REGISTRY_VERSION
                );
            }
            Ok(reg) => Ok(reg),
            Err(e) => anyhow::bail!("Failed to parse {}: {}", path.display(), e),
        }
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&crate::utils::paths::endpoints_file())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        crate::utils::paths::write_public(path, json.as_bytes())
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Endpoint> {
        self.endpoints.get(name)
    }

    pub fn names(&self) -> Vec<String> {
        self.endpoints.keys().cloned().collect()
    }

    /// Endpoints sorted by environment then name, for stable display.
    pub fn sorted(&self) -> Vec<&Endpoint> {
        let mut list: Vec<&Endpoint> = self.endpoints.values().collect();
        list.sort_by(|a, b| {
            a.environment
                .as_deref()
                .unwrap_or("")
                .cmp(b.environment.as_deref().unwrap_or(""))
                .then_with(|| a.name.cmp(&b.name))
        });
        list
    }

    /// Resolve a comma-separated list of names, preserving the given order.
    pub fn select(&self, names: &[String]) -> Result<Vec<Endpoint>> {
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            match self.endpoints.get(trimmed) {
                Some(e) => out.push(e.clone()),
                None => anyhow::bail!(
                    "Unknown endpoint '{}'. Known endpoints: {}",
                    trimmed,
                    if self.endpoints.is_empty() {
                        "(none — add one with `swarmctl endpoint add`)".to_string()
                    } else {
                        self.names().join(", ")
                    }
                ),
            }
        }
        Ok(out)
    }

    /// Every endpoint marked `default`, or all of them if none is.
    pub fn defaults(&self) -> Vec<Endpoint> {
        let marked: Vec<Endpoint> = self
            .endpoints
            .values()
            .filter(|e| e.default)
            .cloned()
            .collect();
        if marked.is_empty() {
            self.sorted().into_iter().cloned().collect()
        } else {
            marked
        }
    }

    /// Add or replace an endpoint, validating it first.
    pub fn upsert(&mut self, endpoint: Endpoint) -> Result<()> {
        validate(&endpoint)?;
        if endpoint.default {
            // Only one endpoint may be the default.
            for e in self.endpoints.values_mut() {
                e.default = false;
            }
        }
        self.endpoints.insert(endpoint.name.clone(), endpoint);
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<Endpoint> {
        self.endpoints
            .remove(name)
            .ok_or_else(|| anyhow::anyhow!("Endpoint '{}' not found", name))
    }

    /// Import Docker contexts that are not already registered.
    ///
    /// Returns the names that were added.
    pub fn import_docker_contexts(&mut self) -> Result<Vec<String>> {
        let contexts = crate::api::context::list_contexts().unwrap_or_default();
        let mut added = Vec::new();
        for ctx in contexts {
            if ctx.name == "default" || self.endpoints.contains_key(&ctx.name) {
                continue;
            }
            let host = ctx.host.clone();
            // Docker's default context has no real host behind a name we can reuse.
            if host.is_empty() {
                continue;
            }
            self.endpoints.insert(
                ctx.name.clone(),
                Endpoint {
                    name: ctx.name.clone(),
                    host,
                    description: if ctx.description.is_empty() {
                        None
                    } else {
                        Some(ctx.description.clone())
                    },
                    site: None,
                    environment: None,
                    tags: vec!["imported".to_string()],
                    tls: ctx.tls.clone(),
                    default: false,
                },
            );
            added.push(ctx.name);
        }
        Ok(added)
    }

    /// Write Docker-compatible context directories for every registered endpoint.
    ///
    /// Docker names the directory `sha256(name)` and keeps TLS material under
    /// `contexts/tls/<digest>/docker/`, so reproducing that layout is what makes
    /// `docker --context <name>` work.
    pub fn sync_to_docker(&self) -> Result<Vec<String>> {
        let mut written = Vec::new();
        for endpoint in self.endpoints.values() {
            if write_docker_context(endpoint).is_ok() {
                written.push(endpoint.name.clone());
            }
        }
        Ok(written)
    }

    /// Remove Docker contexts belonging to endpoints no longer registered.
    pub fn prune_docker_contexts(&self) -> Result<Vec<String>> {
        let docker = crate::api::context::list_contexts().unwrap_or_default();
        let mut removed = Vec::new();
        for ctx in docker {
            if ctx.name == "default" {
                continue;
            }
            if !self.endpoints.contains_key(&ctx.name) {
                let _ = remove_docker_context(&ctx.name);
                removed.push(ctx.name);
            }
        }
        Ok(removed)
    }
}

/// Validate an endpoint before it is stored.
///
/// Catching a malformed host here means a bad entry cannot sit in the registry
/// until the first command that tries to use it.
pub fn validate(endpoint: &Endpoint) -> Result<()> {
    if endpoint.name.trim().is_empty() {
        anyhow::bail!("Endpoint name must not be empty");
    }
    if endpoint.name.contains('/') || endpoint.name.contains(char::is_whitespace) {
        anyhow::bail!(
            "Endpoint name '{}' must not contain whitespace or '/'",
            endpoint.name
        );
    }
    let scheme_ok = ["tcp://", "ssh://", "unix://", "https://"]
        .iter()
        .any(|p| endpoint.host.starts_with(p));
    if !scheme_ok {
        anyhow::bail!(
            "Endpoint host '{}' must start with tcp://, ssh://, unix:// or https://",
            endpoint.host
        );
    }

    // Docker TLS requires all three files together; a partial set fails at
    // connect time with a confusing error.
    if let Some(tls) = &endpoint.tls {
        let missing: Vec<&str> = [
            ("--tlscacert", tls.ca_file.is_none()),
            ("--tlscert", tls.cert_file.is_none()),
            ("--tlskey", tls.key_file.is_none()),
        ]
        .into_iter()
        .filter_map(|(flag, missing)| missing.then_some(flag))
        .collect();
        if !missing.is_empty() {
            anyhow::bail!(
                "Incomplete TLS configuration for '{}': {} not set. Docker requires the CA, \
                 client certificate and client key together.",
                endpoint.name,
                missing.join(", ")
            );
        }
    } else if endpoint.host.starts_with("tcp://") {
        let port_is_tls = endpoint
            .host
            .rsplit(':')
            .next()
            .and_then(|p| p.trim_end_matches('/').parse::<u16>().ok())
            == Some(2376);
        if port_is_tls {
            log::warn!(
                "Endpoint '{}' uses the TLS port 2376 but has no TLS configuration; \
                 connection will fail. Add --tlscacert/--tlscert/--tlskey, or use ssh://.",
                endpoint.name
            );
        }
    }

    if let Some(env) = &endpoint.environment {
        if ![
            "prod",
            "production",
            "staging",
            "stage",
            "dev",
            "test",
            "lab",
        ]
        .contains(&env.as_str())
        {
            log::warn!(
                "Endpoint '{}' has an unrecognised environment '{}' (expected one of \
                 prod, staging, dev, test, lab)",
                endpoint.name,
                env
            );
        }
    }

    Ok(())
}

/// Docker's context directory name for a context: `sha256(name)`.
pub fn docker_context_digest(name: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Materialise a Docker-compatible context for `endpoint`.
///
/// Returns the directory written. TLS material is copied into
/// `contexts/tls/<digest>/docker/` so the paths in `meta.json` resolve.
pub fn write_docker_context(endpoint: &Endpoint) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let docker_dir = crate::api::docker_check::docker_config_dir()
        .context("Could not locate the Docker configuration directory")?;
    let digest = docker_context_digest(&endpoint.name);
    let meta_dir = docker_dir.join("contexts").join("meta").join(&digest);
    std::fs::create_dir_all(&meta_dir)
        .with_context(|| format!("Failed to create {}", meta_dir.display()))?;

    let mut tls_json = serde_json::Map::new();
    if let Some(tls) = &endpoint.tls {
        let tls_dir = docker_dir
            .join("contexts")
            .join("tls")
            .join(&digest)
            .join("docker");
        std::fs::create_dir_all(&tls_dir)
            .with_context(|| format!("Failed to create {}", tls_dir.display()))?;
        // Restrict the directory: it holds private key material.
        let _ = std::fs::set_permissions(&tls_dir, std::fs::Permissions::from_mode(0o700));

        let copy = |field: &Option<String>, filename: &str| -> Result<String> {
            let Some(src) = field else {
                return Ok(String::new());
            };
            let src_path = std::path::Path::new(src);
            let dst = tls_dir.join(filename);
            if src_path.exists() {
                std::fs::copy(src_path, &dst)
                    .with_context(|| format!("Failed to copy {} to {}", src, dst.display()))?;
                let _ = std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o600));
            }
            Ok(dst.to_string_lossy().into_owned())
        };

        let ca = copy(&tls.ca_file, "ca.pem")?;
        let cert = copy(&tls.cert_file, "cert.pem")?;
        let key = copy(&tls.key_file, "key.pem")?;
        tls_json.insert("CAFile".into(), serde_json::json!(ca));
        tls_json.insert("CertFile".into(), serde_json::json!(cert));
        tls_json.insert("KeyFile".into(), serde_json::json!(key));
    }

    let mut docker_endpoint = serde_json::Map::new();
    docker_endpoint.insert("Host".into(), serde_json::json!(endpoint.host));
    // swarmctl always verifies, so this is always false.
    docker_endpoint.insert("SkipTLSVerify".into(), serde_json::json!(false));
    if !tls_json.is_empty() {
        docker_endpoint.insert("TLS".into(), serde_json::Value::Object(tls_json));
    }

    let mut meta = serde_json::Map::new();
    meta.insert("Name".into(), serde_json::json!(endpoint.name));
    meta.insert(
        "Metadata".into(),
        serde_json::json!({
            "Description": endpoint.description.clone().unwrap_or_else(|| {
                format!("Managed by swarmctl ({})", endpoint.host)
            }),
        }),
    );
    meta.insert(
        "Endpoints".into(),
        serde_json::json!({ "docker": serde_json::Value::Object(docker_endpoint) }),
    );

    let meta_path = meta_dir.join("meta.json");
    let content = serde_json::to_string_pretty(&serde_json::Value::Object(meta))?;
    crate::utils::paths::write_public(&meta_path, content.as_bytes())
        .with_context(|| format!("Failed to write {}", meta_path.display()))?;
    Ok(meta_dir)
}

/// Delete the Docker context directories belonging to `name`.
pub fn remove_docker_context(name: &str) -> Result<()> {
    let Some(docker_dir) = crate::api::docker_check::docker_config_dir() else {
        return Ok(());
    };
    let digest = docker_context_digest(name);
    for sub in ["meta", "tls"] {
        let dir = docker_dir.join("contexts").join(sub).join(&digest);
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
    // Drop the context from Docker's active setting if it pointed here.
    if let Ok(Some(current)) = crate::api::context::get_current_context_name()
        && current == name
    {
        let _ = crate::api::context::set_current_context("default");
    }
    Ok(())
}

/// Endpoint name to file credentials under when none was given.
///
/// Prefers a registered endpoint whose host matches the current connection, so
/// `swarmctl discover` and `swarmctl swarm token` land in the same place as
/// `swarmctl vault store`. Falls back to a sanitised host or context name.
pub fn current_endpoint_key() -> String {
    let host = std::env::var("DOCKER_HOST").ok().filter(|h| !h.is_empty());

    if let Ok(reg) = Registry::load() {
        // An explicitly selected context is the strongest signal.
        let context = std::env::var("DOCKER_CONTEXT")
            .ok()
            .filter(|c| !c.is_empty())
            .or_else(|| {
                crate::api::context::get_current_context_name()
                    .ok()
                    .flatten()
            });

        if let Some(name) = context
            && reg.endpoints.contains_key(&name)
        {
            return name;
        }

        if let Some(host) = &host
            && let Some(found) = reg.endpoints.values().find(|e| e.host == *host)
        {
            return found.name.clone();
        }
    }

    if let Some(name) = std::env::var("DOCKER_CONTEXT")
        .ok()
        .filter(|c| !c.is_empty())
    {
        return sanitize_key(&name);
    }
    match host {
        Some(h) => sanitize_key(&h),
        None => "local".to_string(),
    }
}

/// Reduce a host or name to `[A-Za-z0-9._-]`, never empty.
pub fn sanitize_key(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "default".to_string()
    } else {
        trimmed
    }
}

/// Turn a registry endpoint into a connection config.
pub fn to_connection_config(endpoint: &Endpoint) -> crate::api::client::ConnectionConfig {
    let mut config = crate::api::client::ConnectionConfig {
        host: Some(endpoint.host.clone()),
        ..Default::default()
    };
    if let Some(tls) = &endpoint.tls {
        config.tlscacert = tls.ca_file.clone();
        config.tlscert = tls.cert_file.clone();
        config.tlskey = tls.key_file.clone();
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(name: &str, host: &str) -> Endpoint {
        Endpoint {
            name: name.into(),
            host: host.into(),
            description: None,
            site: None,
            environment: None,
            tags: Vec::new(),
            tls: None,
            default: false,
        }
    }

    fn registry_with(eps: Vec<Endpoint>) -> Registry {
        let mut r = Registry::default();
        for e in eps {
            r.upsert(e).unwrap();
        }
        r
    }

    #[test]
    fn registry_round_trips_through_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("endpoints.json");
        let mut reg = Registry::default();
        reg.upsert(Endpoint {
            site: Some("eu-west".into()),
            environment: Some("prod".into()),
            tags: vec!["offsite".into()],
            ..endpoint("prod-eu", "ssh://ops@10.8.0.4")
        })
        .unwrap();
        reg.save_to(&path).unwrap();

        let loaded = Registry::load_from(&path).unwrap();
        assert_eq!(loaded.endpoints.len(), 1);
        let e = loaded.get("prod-eu").unwrap();
        assert_eq!(e.host, "ssh://ops@10.8.0.4");
        assert_eq!(e.site.as_deref(), Some("eu-west"));
        assert_eq!(e.environment.as_deref(), Some("prod"));
        assert_eq!(e.tags, vec!["offsite".to_string()]);
    }

    #[test]
    fn missing_file_is_an_empty_registry_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = Registry::load_from(&tmp.path().join("nope.json")).unwrap();
        assert!(reg.endpoints.is_empty());
    }

    #[test]
    fn newer_registry_version_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("endpoints.json");
        std::fs::write(&path, r#"{"version": 999, "endpoints": {}}"#).unwrap();
        let err = Registry::load_from(&path).unwrap_err().to_string();
        assert!(err.contains("newer swarmctl"), "got: {err}");
    }

    #[test]
    fn corrupt_registry_reports_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("endpoints.json");
        std::fs::write(&path, "{ not json").unwrap();
        let err = Registry::load_from(&path).unwrap_err().to_string();
        assert!(err.contains("Failed to parse"), "got: {err}");
    }

    #[test]
    fn rejects_hosts_without_a_supported_scheme() {
        for host in ["10.0.0.1:2376", "example.com", "", "ftp://x"] {
            assert!(
                validate(&endpoint("e", host)).is_err(),
                "should reject {host:?}"
            );
        }
        for host in [
            "tcp://10.0.0.1:2376",
            "ssh://user@host",
            "unix:///var/run/docker.sock",
            "https://docker.example.com",
        ] {
            assert!(
                validate(&endpoint("e", host)).is_ok(),
                "should accept {host:?}"
            );
        }
    }

    #[test]
    fn rejects_names_with_whitespace_or_slashes() {
        assert!(validate(&endpoint("my endpoint", "ssh://h")).is_err());
        assert!(validate(&endpoint("a/b", "ssh://h")).is_err());
        assert!(validate(&endpoint("", "ssh://h")).is_err());
        assert!(validate(&endpoint("prod-eu", "ssh://h")).is_ok());
    }

    #[test]
    fn rejects_partial_tls_configuration() {
        let mut e = endpoint("prod", "tcp://10.0.0.1:2376");
        e.tls = Some(TlsConfig {
            ca_file: Some("/ca.pem".into()),
            cert_file: None,
            key_file: None,
        });
        let err = validate(&e).unwrap_err().to_string();
        assert!(err.contains("--tlscert"), "got: {err}");
        assert!(err.contains("--tlskey"), "got: {err}");
    }

    #[test]
    fn accepts_complete_tls_configuration() {
        let mut e = endpoint("prod", "tcp://10.0.0.1:2376");
        e.tls = Some(TlsConfig {
            ca_file: Some("/ca.pem".into()),
            cert_file: Some("/cert.pem".into()),
            key_file: Some("/key.pem".into()),
        });
        assert!(validate(&e).is_ok());
    }

    #[test]
    fn only_one_endpoint_can_be_the_default() {
        let mut reg = Registry::default();
        let mut a = endpoint("a", "ssh://a");
        a.default = true;
        let mut b = endpoint("b", "ssh://b");
        b.default = true;
        reg.upsert(a).unwrap();
        reg.upsert(b).unwrap();

        assert!(!reg.get("a").unwrap().default);
        assert!(reg.get("b").unwrap().default);
        assert_eq!(reg.defaults().len(), 1);
    }

    #[test]
    fn defaults_falls_back_to_everything() {
        let reg = registry_with(vec![endpoint("a", "ssh://a"), endpoint("b", "ssh://b")]);
        assert_eq!(reg.defaults().len(), 2);
    }

    #[test]
    fn select_preserves_order_and_reports_unknown_names() {
        let reg = registry_with(vec![
            endpoint("a", "ssh://a"),
            endpoint("b", "ssh://b"),
            endpoint("c", "ssh://c"),
        ]);
        let picked = reg.select(&["c".into(), "a".into()]).unwrap();
        assert_eq!(
            picked.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["c", "a"]
        );

        let err = reg.select(&["nope".into()]).unwrap_err().to_string();
        assert!(err.contains("Unknown endpoint 'nope'"), "got: {err}");
        assert!(err.contains("a, b, c"), "should list known names: {err}");
    }

    #[test]
    fn select_error_on_empty_registry_is_actionable() {
        let reg = Registry::default();
        let err = reg.select(&["x".into()]).unwrap_err().to_string();
        assert!(err.contains("endpoint add"), "got: {err}");
    }

    #[test]
    fn select_skips_blank_entries() {
        let reg = registry_with(vec![endpoint("a", "ssh://a")]);
        let picked = reg.select(&["".into(), "  ".into(), "a".into()]).unwrap();
        assert_eq!(picked.len(), 1);
    }

    #[test]
    fn remove_reports_unknown_names() {
        let mut reg = registry_with(vec![endpoint("a", "ssh://a")]);
        assert!(reg.remove("a").is_ok());
        assert!(reg.remove("a").is_err());
    }

    #[test]
    fn sorted_orders_by_environment_then_name() {
        let mut prod1 = endpoint("prod-b", "ssh://b");
        prod1.environment = Some("prod".into());
        let mut prod2 = endpoint("prod-a", "ssh://a");
        prod2.environment = Some("prod".into());
        let mut dev1 = endpoint("dev-a", "ssh://d");
        dev1.environment = Some("dev".into());

        let reg = registry_with(vec![prod1, prod2, dev1]);
        let names: Vec<&str> = reg.sorted().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["dev-a", "prod-a", "prod-b"]);
    }

    #[test]
    fn docker_digest_matches_dockers_layout() {
        // Verified against `docker context create`: the directory Docker creates
        // is sha256 of the context name, in lowercase hex.
        assert_eq!(
            docker_context_digest("swarmctl-layout-probe"),
            "e61a25b5481f59600d27b43c275225e69fcf9113f123ac4e72df019502c9ad80"
        );
    }

    #[test]
    fn docker_digest_is_stable_and_name_specific() {
        assert_eq!(docker_context_digest("prod"), docker_context_digest("prod"));
        assert_ne!(docker_context_digest("prod"), docker_context_digest("dev"));
        assert_eq!(docker_context_digest("x").len(), 64);
    }

    #[test]
    fn sanitize_key_never_returns_empty() {
        assert_eq!(sanitize_key("///"), "default");
        assert_eq!(sanitize_key(""), "default");
        assert_eq!(sanitize_key("tcp://10.0.0.1:2376"), "tcp---10.0.0.1-2376");
    }

    #[test]
    fn current_endpoint_key_falls_back_without_a_registry() {
        // No registry on disk in the test environment, and no DOCKER_HOST set by
        // the test runner, so this must still produce a usable key.
        let key = current_endpoint_key();
        assert!(!key.is_empty());
    }

    #[test]
    fn connection_config_carries_tls_paths() {
        let mut e = endpoint("prod", "tcp://10.0.0.1:2376");
        e.tls = Some(TlsConfig {
            ca_file: Some("/ca.pem".into()),
            cert_file: Some("/cert.pem".into()),
            key_file: Some("/key.pem".into()),
        });
        let cfg = to_connection_config(&e);
        assert_eq!(cfg.host.as_deref(), Some("tcp://10.0.0.1:2376"));
        assert_eq!(cfg.tlscacert.as_deref(), Some("/ca.pem"));
        assert_eq!(cfg.tlskey.as_deref(), Some("/key.pem"));
        assert!(!cfg.skip_tls_verify);
    }

    #[test]
    fn connection_config_without_tls_is_plain() {
        let cfg = to_connection_config(&endpoint("lab", "ssh://lab"));
        assert_eq!(cfg.host.as_deref(), Some("ssh://lab"));
        assert!(!cfg.has_tls());
    }
}
