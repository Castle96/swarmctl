//! Last-applied manifest store, keyed per endpoint.
//!
//! Swarm has no admission controllers and no versioned objects, so nothing in
//! the ecosystem records what a cluster was *asked* to run. `apply` writes the
//! manifest it submitted here; `drift` reads it back and compares it against
//! the live object. Without this store there is no way to tell a manual edit
//! or a moved image tag apart from the declared state.
//!
//! Layout, mirroring the endpoint registry:
//!   `~/.swarmctl/applied/<endpoint>/<kind>/<name>.json`
//! The endpoint segment keeps several clusters from sharing one namespace.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One stored application of a resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppliedRecord {
    /// The manifest exactly as submitted.
    pub manifest: serde_json::Value,
    /// Unix timestamp of the apply.
    pub applied_at: u64,
    /// The endpoint the manifest was applied to.
    pub endpoint: String,
}

/// Root of the store on disk.
pub fn applied_dir() -> PathBuf {
    base_dir_at(&crate::utils::paths::swarmctl_dir())
}

/// Store root under an explicit base directory.
pub fn base_dir_at(base: &Path) -> PathBuf {
    base.join("applied")
}

/// Directory holding every recorded manifest for one endpoint.
pub fn endpoint_dir(endpoint: &str) -> PathBuf {
    applied_dir().join(endpoint)
}

/// Directory holding one resource kind's manifests for one endpoint.
pub fn kind_dir(endpoint: &str, kind: &str) -> PathBuf {
    endpoint_dir(endpoint).join(kind)
}

/// Path of the stored manifest for one resource.
///
/// The name is used verbatim; `record` rejects names that would escape the
/// directory so a manifest named `../../vault` cannot write outside the store.
pub fn record_path(endpoint: &str, kind: &str, name: &str) -> PathBuf {
    kind_dir(endpoint, kind).join(format!("{}.json", name))
}

/// Path of the stored manifest under an explicit base directory.
pub fn record_path_at(base: &Path, endpoint: &str, kind: &str, name: &str) -> PathBuf {
    base_dir_at(base)
        .join(endpoint)
        .join(kind)
        .join(format!("{}.json", name))
}

/// Whether a resource name is safe to use as a path segment.
pub fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

/// Record a manifest as the declared state of one resource.
pub fn record(endpoint: &str, kind: &str, name: &str, manifest: serde_json::Value) -> Result<()> {
    record_at(
        &crate::utils::paths::swarmctl_dir(),
        endpoint,
        kind,
        name,
        manifest,
    )
}

/// Record a manifest under an explicit base directory.
pub fn record_at(
    base: &Path,
    endpoint: &str,
    kind: &str,
    name: &str,
    manifest: serde_json::Value,
) -> Result<()> {
    if !is_safe_name(name) {
        anyhow::bail!(
            "Refusing to record manifest for unsafe resource name '{}'",
            name
        );
    }
    let path = record_path_at(base, endpoint, kind, name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let entry = AppliedRecord {
        manifest,
        applied_at: epoch_now(),
        endpoint: endpoint.to_string(),
    };
    let json = serde_json::to_string_pretty(&entry)?;
    // Owner-only: a manifest can reference secrets and carries image tags.
    crate::utils::paths::write_private(&path, json.as_bytes())?;
    Ok(())
}

/// Read the last-applied manifest for one resource, if any.
pub fn read(endpoint: &str, kind: &str, name: &str) -> Result<Option<AppliedRecord>> {
    read_at_path(&record_path(endpoint, kind, name))
}

/// Read the manifest stored under an explicit base directory.
///
/// Used by tests, which point the store at a temp dir.
#[cfg_attr(not(test), allow(dead_code))]
pub fn read_at(
    base: &Path,
    endpoint: &str,
    kind: &str,
    name: &str,
) -> Result<Option<AppliedRecord>> {
    read_at_path(&record_path_at(base, endpoint, kind, name))
}

fn read_at_path(path: &Path) -> Result<Option<AppliedRecord>> {
    if !path.exists() {
        return Ok(None);
    }
    let json = std::fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&json)?))
}

/// Every recorded manifest for one kind, keyed by resource name.
pub fn list(endpoint: &str, kind: &str) -> Result<BTreeMap<String, AppliedRecord>> {
    list_at(&crate::utils::paths::swarmctl_dir(), endpoint, kind)
}

/// Every recorded manifest for one kind under an explicit base directory.
pub fn list_at(base: &Path, endpoint: &str, kind: &str) -> Result<BTreeMap<String, AppliedRecord>> {
    let dir = base_dir_at(base).join(endpoint).join(kind);
    let mut out = BTreeMap::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        // A corrupt or unreadable record must not abort the whole listing:
        // `drift` still has to report the resources it can read.
        match read_at_path(&path) {
            Ok(Some(record)) => {
                out.insert(name.to_string(), record);
            }
            Ok(None) => {}
            Err(e) => log::warn!("skipping unreadable manifest {}: {}", path.display(), e),
        }
    }
    Ok(out)
}

/// Delete the stored manifest for one resource.
///
/// Called on `delete`, so removing a resource does not leave a phantom that
/// `drift` would report as missing-from-cluster forever.
pub fn remove(endpoint: &str, kind: &str, name: &str) -> Result<()> {
    remove_at(&crate::utils::paths::swarmctl_dir(), endpoint, kind, name)
}

/// Delete the stored manifest under an explicit base directory.
pub fn remove_at(base: &Path, endpoint: &str, kind: &str, name: &str) -> Result<()> {
    let path = record_path_at(base, endpoint, kind, name);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Which endpoints have any recorded manifests.
///
/// Used by `drift forget --all` to find every cluster with state to clear.
pub fn endpoints_with_state() -> Result<Vec<String>> {
    endpoints_with_state_at(&crate::utils::paths::swarmctl_dir())
}

/// Which endpoints have recorded manifests under an explicit base directory.
///
/// Used by tests, which point the store at a temp dir.
#[cfg_attr(not(test), allow(dead_code))]
pub fn endpoints_with_state_at(base: &Path) -> Result<Vec<String>> {
    let dir = base_dir_at(base);
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.path().is_dir()
            && let Some(name) = entry.file_name().to_str()
        {
            out.push(name.to_string());
        }
    }
    out.sort();
    Ok(out)
}

fn epoch_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A fresh temp dir and a unique endpoint name inside it.
    ///
    /// The base directory is passed explicitly rather than through
    /// `SWARMCTL_DIR`, so tests running in parallel cannot clobber each
    /// other's state.
    fn tmp() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = format!("test-{}", std::process::id());
        (dir, endpoint)
    }

    #[test]
    fn is_safe_name_rejects_path_traversal() {
        assert!(is_safe_name("web"));
        assert!(is_safe_name("my-app.1"));
        assert!(!is_safe_name(""));
        assert!(!is_safe_name("."));
        assert!(!is_safe_name(".."));
        assert!(!is_safe_name("../vault"));
        assert!(!is_safe_name("a/b"));
        assert!(!is_safe_name("a\\b"));
    }

    #[test]
    fn record_then_read_round_trips() {
        let (dir, ep) = tmp();
        record_at(
            dir.path(),
            &ep,
            "services",
            "web",
            json!({"Image": "nginx:1.25"}),
        )
        .unwrap();
        let got = read_at(dir.path(), &ep, "services", "web")
            .unwrap()
            .unwrap();
        assert_eq!(got.manifest, json!({"Image": "nginx:1.25"}));
        assert_eq!(got.endpoint, ep);
        assert!(got.applied_at > 0);
    }

    #[test]
    fn record_refuses_an_unsafe_name() {
        let (dir, ep) = tmp();
        let err = record_at(dir.path(), &ep, "services", "../../vault", json!({}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("unsafe"), "got: {err}");
    }

    #[test]
    fn list_returns_every_recorded_resource() {
        let (dir, ep) = tmp();
        record_at(dir.path(), &ep, "services", "web", json!({"Image": "a"})).unwrap();
        record_at(dir.path(), &ep, "services", "api", json!({"Image": "b"})).unwrap();
        let all = list_at(dir.path(), &ep, "services").unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.contains_key("web"));
        assert!(all.contains_key("api"));
    }

    #[test]
    fn list_of_an_unknown_endpoint_is_empty_not_an_error() {
        let (dir, ep) = tmp();
        assert!(
            list_at(dir.path(), &format!("{ep}-nowhere"), "services")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn records_are_scoped_to_one_endpoint() {
        let (dir, ep) = tmp();
        record_at(
            dir.path(),
            &format!("{ep}-prod"),
            "services",
            "web",
            json!({}),
        )
        .unwrap();
        record_at(
            dir.path(),
            &format!("{ep}-staging"),
            "services",
            "web",
            json!({}),
        )
        .unwrap();
        assert_eq!(
            list_at(dir.path(), &format!("{ep}-prod"), "services")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            list_at(dir.path(), &format!("{ep}-staging"), "services")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn endpoints_with_state_lists_the_populated_endpoints() {
        let (dir, ep) = tmp();
        record_at(
            dir.path(),
            &format!("{ep}-prod"),
            "services",
            "web",
            json!({}),
        )
        .unwrap();
        record_at(
            dir.path(),
            &format!("{ep}-staging"),
            "services",
            "web",
            json!({}),
        )
        .unwrap();
        assert_eq!(endpoints_with_state_at(dir.path()).unwrap().len(), 2);
    }

    #[test]
    fn remove_drops_only_that_record() {
        let (dir, ep) = tmp();
        record_at(dir.path(), &ep, "services", "web", json!({})).unwrap();
        record_at(dir.path(), &ep, "services", "api", json!({})).unwrap();
        remove_at(dir.path(), &ep, "services", "web").unwrap();
        let all = list_at(dir.path(), &ep, "services").unwrap();
        assert_eq!(all.len(), 1);
        assert!(all.contains_key("api"));
    }

    #[test]
    fn remove_of_an_absent_record_is_not_an_error() {
        let (dir, ep) = tmp();
        assert!(remove_at(dir.path(), &ep, "services", "absent").is_ok());
    }

    #[test]
    fn record_overwrites_the_previous_manifest() {
        let (dir, ep) = tmp();
        record_at(dir.path(), &ep, "services", "web", json!({"Image": "a"})).unwrap();
        record_at(dir.path(), &ep, "services", "web", json!({"Image": "b"})).unwrap();
        let got = read_at(dir.path(), &ep, "services", "web")
            .unwrap()
            .unwrap();
        assert_eq!(got.manifest, json!({"Image": "b"}));
        assert_eq!(list_at(dir.path(), &ep, "services").unwrap().len(), 1);
    }

    #[test]
    fn endpoints_with_state_is_empty_without_the_directory() {
        let (dir, _ep) = tmp();
        assert!(endpoints_with_state_at(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn record_path_scopes_by_endpoint_and_kind() {
        let p = record_path("prod-eu", "services", "web");
        assert!(
            p.ends_with("applied/prod-eu/services/web.json"),
            "got: {:?}",
            p
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_recorded_manifest_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, ep) = tmp();
        record_at(dir.path(), &ep, "services", "web", json!({})).unwrap();
        let mode = std::fs::metadata(record_path_at(dir.path(), &ep, "services", "web"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "a manifest can reference secrets");
    }
}
