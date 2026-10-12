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
    if let Ok(override_dir) = std::env::var("SWARMCTL_DIR")
        && !override_dir.is_empty()
    {
        return PathBuf::from(override_dir);
    }
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".swarmctl"))
        .unwrap_or_else(|| PathBuf::from(".swarmctl"))
}

/// Endpoint registry file.
///
/// Used by the endpoint registry; kept here so every swarmctl path is defined in
/// one place.
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
/// Create `dir` with owner-only permissions.
pub fn ensure_swarmctl_dir_at(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    restrict(dir)?;
    Ok(())
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

/// Write a file readable and writable only by its owner, atomically.
///
/// The contents go to a temporary file in the same directory, are flushed to
/// disk, and are then renamed over the target. A rename within a filesystem is
/// atomic, so a crash or a full disk can no longer leave a half-written vault:
/// the previous contents survive intact. The old code opened the target with
/// `truncate(true)` and wrote in place, which could destroy every stored
/// credential if it died halfway.
#[cfg(unix)]
pub fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        restrict(parent)?;
    }

    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let tmp = parent.join(format!(
        ".{}.tmp{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("state"),
        std::process::id()
    ));

    let write_result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(contents)?;
        // Durable before the rename, so a power loss cannot leave a
        // correctly-named file with unflushed contents.
        file.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

/// Write a non-secret file under the swarmctl state directory.
///
/// The parent directory is still restricted to the owner: it is shared with the
/// vault, so leaving it world-readable would weaken the vault file's own 0600.
pub fn write_public(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        ensure_swarmctl_dir_at(parent)?;
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
    fn public_write_still_restricts_the_shared_directory() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".swarmctl");
        let path = dir.join("endpoints.json");
        write_public(&path, b"{}").unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o700,
            "the state directory is shared with the vault and must stay private"
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_write_replaces_existing_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secret.json");
        write_private(&path, b"a very long previous value").unwrap();
        write_private(&path, b"{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
    }

    /// The atomic write must not leave its scratch file behind on success, or
    /// the vault directory accumulates `.<name>.tmp<pid>` files holding
    /// decrypted-adjacent material.
    #[cfg(unix)]
    #[test]
    fn private_write_leaves_no_temporary_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secret.json");
        write_private(&path, b"payload").unwrap();

        let leftovers: Vec<String> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "secret.json")
            .collect();
        assert!(leftovers.is_empty(), "unexpected files: {leftovers:?}");
    }

    /// A failed write must leave the previous contents intact. The old
    /// truncate-then-write could leave a half-written vault, destroying every
    /// stored credential; the rename is atomic, so the old file survives.
    #[cfg(unix)]
    #[test]
    fn a_failed_write_preserves_the_previous_file() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secret.json");
        write_private(&path, b"the good contents").unwrap();

        // Make the directory read-only so the temp file cannot be created.
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = write_private(&path, b"replacement");
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();

        // Either the write succeeded (running as root, which ignores the mode)
        // or it failed cleanly. In both cases the file must parse.
        if result.is_err() {
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                "the good contents",
                "a failed write must not truncate the existing file"
            );
        } else {
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");
        }
    }

    /// The written file must stay owner-only even though it is created as a
    /// temporary and renamed into place.
    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secret.json");
        write_private(&path, b"payload").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "got {:o}", mode & 0o777);
    }
}
