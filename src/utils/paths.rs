//! swarmctl's own on-disk locations.
//!
//! Everything the tool stores for itself lives under a single directory so it is
//! easy to locate, back up, and reason about permissions for. Docker's own
//! configuration is a separate concern and lives in [`docker_config_dir`].

use std::path::PathBuf;

/// Directory holding swarmctl state: endpoint registry, vault, settings.
///
/// `~/.swarmctl` on Unix; `%USERPROFILE%\.swarmctl` on Windows.
pub fn swarmctl_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".swarmctl"))
        .unwrap_or_else(|| PathBuf::from(".swarmctl"))
}

/// Endpoint registry file.
///
/// Used by the endpoint registry; kept here so every swarmctl path is defined in
/// one place.
#[allow(dead_code)]
pub fn endpoints_file() -> PathBuf {
    swarmctl_dir().join("endpoints.json")
}

/// Encrypted credential store.
pub fn vault_file() -> PathBuf {
    swarmctl_dir().join("vault.json")
}

/// Persisted failover monitoring flag.
pub fn failover_file() -> PathBuf {
    swarmctl_dir().join("failover.json")
}

/// Create `swarmctl_dir()` with owner-only permissions and return it.
///
/// The directory holds the encrypted vault; on a shared machine a world-readable
/// directory would expose it even if the file mode is tight.
pub fn ensure_swarmctl_dir() -> std::io::Result<PathBuf> {
    let dir = swarmctl_dir();
    std::fs::create_dir_all(&dir)?;
    restrict(&dir)?;
    Ok(dir)
}

#[cfg(unix)]
fn restrict(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict(_dir: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

/// Write a file readable and writable only by its owner.
#[cfg(unix)]
pub fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        restrict(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(contents)
}

#[cfg(not(unix))]
pub fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

/// Write a file readable by anyone (used only for non-secret data).
#[allow(dead_code)]
pub fn write_public(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_state_lives_under_one_directory() {
        let dir = swarmctl_dir();
        for path in [endpoints_file(), vault_file(), failover_file()] {
            assert!(
                path.starts_with(&dir),
                "{} should live under {}",
                path.display(),
                dir.display()
            );
        }
    }

    #[test]
    fn files_are_distinct() {
        let files = [endpoints_file(), vault_file(), failover_file()];
        for (i, a) in files.iter().enumerate() {
            for b in files.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secret.json");
        write_private(&path, b"{}").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "got {:o}", mode & 0o777);
    }

    #[cfg(unix)]
    #[test]
    fn private_write_creates_missing_parents() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested/deeper/secret.json");
        write_private(&path, b"{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
    }

    #[cfg(unix)]
    #[test]
    fn private_write_truncates_existing_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secret.json");
        write_private(&path, b"a very long previous value").unwrap();
        write_private(&path, b"{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
    }
}
