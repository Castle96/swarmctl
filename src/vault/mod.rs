pub mod crypto;
pub mod models;

use anyhow::{Context, Result};
use models::{
    EndpointSecrets, JoinTokens, KdfParams, LegacyVaultData, VAULT_VERSION, VaultData, VaultFile,
};
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

/// Encrypted local credential store.
///
/// Credentials are keyed by endpoint name so several clusters can be held at
/// once; v1 vaults (a single cluster, stored as top-level fields) are migrated on
/// first open.
pub struct LocalVault {
    path: PathBuf,
    data: VaultData,
    password: String,
    /// Key-derivation parameters this vault was written with.
    ///
    /// Carried rather than recomputed on every save, so a vault keeps opening
    /// with the cost it was created at even if the default changes.
    kdf: KdfParams,
}

impl LocalVault {
    /// Vault location under `~/.swarmctl`.
    pub fn vault_path() -> PathBuf {
        crate::utils::paths::vault_file()
    }

    pub fn exists() -> bool {
        Self::vault_path().exists()
    }

    /// Vault location rooted at an explicit directory. Used by tests, and by
    /// callers that manage swarmctl state somewhere other than the default.
    #[allow(dead_code)]
    pub fn at(dir: &Path) -> PathBuf {
        dir.join("vault.json")
    }

    #[allow(dead_code)]
    pub fn exists_at(path: &Path) -> bool {
        path.exists()
    }

    /// Create a new vault, refusing to clobber an existing one.
    pub fn create(password: &str) -> Result<Self> {
        Self::create_at(&Self::vault_path(), password)
    }

    pub fn create_at(path: &Path, password: &str) -> Result<Self> {
        if path.exists() {
            anyhow::bail!(
                "A vault already exists at {}. Use `swarmctl vault set-key` to change the \
                 password, or remove the file to start over.",
                path.display()
            );
        }
        // Ensure the parent exists with owner-only permissions before writing.
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            crate::utils::paths::ensure_swarmctl_dir_at(parent)?;
        }

        let data = VaultData {
            created_at: epoch_now(),
            ..Default::default()
        };
        let vault = Self {
            path: path.to_path_buf(),
            data,
            password: password.to_string(),
            kdf: KdfParams::current(),
        };
        vault.save()?;
        Ok(vault)
    }

    /// Open a vault, migrating a v1 document if necessary.
    pub fn open(password: &str) -> Result<Self> {
        Self::open_at(&Self::vault_path(), password)
    }

    pub fn open_at(path: &Path, password: &str) -> Result<Self> {
        if !path.exists() {
            anyhow::bail!(
                "No vault found at {}. Run `swarmctl vault init` first.",
                path.display()
            );
        }

        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let envelope: VaultFile = serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", path.display()))?;

        // A vault written by a newer swarmctl must be refused rather than
        // reinterpreted. `Registry::load_from` already does this.
        if envelope.version > VAULT_VERSION {
            anyhow::bail!(
                "{} was written by a newer swarmctl (format v{}, this build supports v{}). \
                 Upgrade swarmctl to read it.",
                path.display(),
                envelope.version,
                VAULT_VERSION
            );
        }

        // Vaults written before the KDF was recorded used this build's defaults.
        let kdf = envelope.kdf.unwrap_or_default();

        let mut plaintext = crypto::decrypt_with(
            &kdf,
            &envelope.salt,
            &envelope.nonce,
            &envelope.ciphertext,
            password,
        )
        .map_err(|e| {
            // Keep the KDF explanation only when that is what failed.
            anyhow::anyhow!(
                "{}. ({}) has not been modified.",
                if e.to_string().contains("key derivation") {
                    "This vault's key-derivation parameters are not supported"
                } else {
                    "Could not decrypt the vault. Wrong password?"
                },
                path.display()
            )
        })?;

        let (data, migrated) = decode_payload(&plaintext, path)?;
        // The decrypted join tokens are the sensitive material; scrub the
        // serialized copy once the structured data exists.
        plaintext.zeroize();

        let vault = Self {
            path: path.to_path_buf(),
            data,
            password: password.to_string(),
            kdf,
        };

        if migrated {
            eprintln!(
                "Migrated vault from format v1 to v{} ({} endpoint(s)).",
                VAULT_VERSION,
                vault.data.endpoint_names().len()
            );
            vault.save()?;
        }

        Ok(vault)
    }

    pub fn save(&self) -> Result<()> {
        let plaintext = serde_json::to_vec(&self.data).context("Failed to serialize vault data")?;
        let (salt, nonce, ciphertext) = crypto::encrypt(&plaintext, &self.password)?;
        let envelope = VaultFile {
            version: VAULT_VERSION,
            // Recorded so a future change to the KDF cannot silently make
            // existing vaults undecryptable.
            kdf: Some(self.kdf.clone()),
            salt,
            nonce,
            ciphertext,
        };
        let json = serde_json::to_string_pretty(&envelope)?;
        crate::utils::paths::write_private(&self.path, json.as_bytes())
            .with_context(|| format!("Failed to write {}", self.path.display()))?;
        Ok(())
    }

    pub fn data(&self) -> &VaultData {
        &self.data
    }

    /// Mutable access to the decrypted store, used by the tests to seed state.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn data_mut(&mut self) -> &mut VaultData {
        &mut self.data
    }

    /// Store join tokens for one endpoint.
    pub fn store_swarm_tokens(
        &mut self,
        endpoint: &str,
        tokens: JoinTokens,
        unlock_key: Option<String>,
        docker_host: &str,
        swarm_name: &str,
    ) -> Result<()> {
        if endpoint.trim().is_empty() {
            anyhow::bail!("Endpoint name must not be empty");
        }
        let updated_at = epoch_now();
        let existing_nodes = self
            .data
            .get(endpoint)
            .map(|e| e.nodes.clone())
            .unwrap_or_default();

        self.data.store(
            endpoint,
            EndpointSecrets {
                swarm_name: swarm_name.to_string(),
                join_tokens: tokens,
                unlock_key,
                nodes: existing_nodes,
                docker_host: docker_host.to_string(),
                updated_at,
            },
        );
        self.save()
    }

    /// Replace only the tokens, keeping other recorded metadata.
    pub fn rotate_tokens(&mut self, endpoint: &str, tokens: JoinTokens) -> Result<()> {
        let entry = self.data.get(endpoint).cloned().unwrap_or_default();
        // Spelled out rather than using `..entry`: `EndpointSecrets` has a
        // `Drop` impl (to zeroize), and struct update syntax cannot move fields
        // out of a type that implements Drop.
        let rotated = EndpointSecrets {
            updated_at: epoch_now(),
            join_tokens: tokens,
            swarm_name: entry.swarm_name,
            unlock_key: entry.unlock_key,
            nodes: entry.nodes,
            docker_host: entry.docker_host,
        };
        self.data.store(endpoint, rotated);
        self.save()
    }

    pub fn remove_endpoint(&mut self, endpoint: &str) -> Result<EndpointSecrets> {
        let removed = self
            .data
            .remove(endpoint)
            .ok_or_else(|| anyhow::anyhow!("No credentials stored for endpoint '{}'", endpoint))?;
        self.save()?;
        Ok(removed)
    }

    /// Change the password: re-encrypt with a fresh salt and nonce.
    pub fn change_password(&mut self, new_password: &str) -> Result<()> {
        let mut previous = std::mem::replace(&mut self.password, new_password.to_string());
        let result = self.save();
        if result.is_err() {
            // Keep the old password so the caller can retry rather than being
            // left unable to save.
            self.password = std::mem::take(&mut previous);
        }
        // Zeroized on both paths. The old copy used to survive the failure
        // branch, leaving the previous password in an ordinary heap string.
        previous.zeroize();
        result
    }

    /// Record a node against an endpoint's entry.
    #[allow(dead_code)]
    pub fn record_node(&mut self, endpoint: &str, node: models::NodeRecord) -> Result<()> {
        let mut entry = self.data.get(endpoint).cloned().unwrap_or_default();
        entry.nodes.retain(|n| n.node_id != node.node_id);
        entry.nodes.push(node);
        entry.updated_at = epoch_now();
        self.data.store(endpoint, entry);
        self.save()
    }

    /// Aggregate vault state for `vault status` / `vault ls`.
    pub fn status(&self) -> models::VaultStatus {
        models::VaultStatus {
            created_at: self.data.created_at.clone(),
            total_endpoints: self.data.endpoints.len(),
            endpoints: self
                .data
                .endpoints
                .iter()
                .filter(|(_, e)| e.has_credentials())
                .map(|(name, _)| name.clone())
                .collect(),
        }
    }

    /// One row per endpoint, for `vault ls`.
    pub fn rows(&self) -> Vec<models::VaultEntryRow> {
        self.data
            .endpoints
            .iter()
            .map(|(name, e)| models::VaultEntryRow {
                endpoint: name.clone(),
                swarm: if e.swarm_name.is_empty() {
                    "-".to_string()
                } else {
                    e.swarm_name.clone()
                },
                has_worker_token: !e.join_tokens.worker.is_empty(),
                has_manager_token: !e.join_tokens.manager.is_empty(),
                unlock_key: e.unlock_key.clone(),
                nodes: e.nodes.len(),
                updated_at: if e.updated_at.is_empty() {
                    None
                } else {
                    Some(e.updated_at.clone())
                },
            })
            .collect()
    }
}

impl std::fmt::Debug for LocalVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render the password.
        f.debug_struct("LocalVault")
            .field("path", &self.path)
            .field("endpoints", &self.data.endpoint_names())
            .field("password", &"<redacted>")
            .finish()
    }
}

impl Drop for LocalVault {
    fn drop(&mut self) {
        self.password.zeroize();
        // The decrypted join tokens and the manager unlock key live in `data`
        // for the whole lifetime of the vault. The derived key and the password
        // were already being scrubbed, so leaving the plaintext was the one gap
        // in an otherwise careful design.
        self.data.zeroize();
    }
}

/// Decrypt a payload into the v2 structure, migrating v1 content when detected.
///
/// Returns the data and whether the file needs rewriting. A v2 document with no
/// endpoints is legitimately empty, so an empty result is not by itself evidence
/// of a v1 file — the legacy shape is checked explicitly.
fn decode_payload(plaintext: &[u8], path: &Path) -> Result<(VaultData, bool)> {
    match serde_json::from_slice::<VaultData>(plaintext) {
        Ok(data) if !data.endpoints.is_empty() => Ok((data, false)),
        Ok(data) => {
            let looks_v1 = serde_json::from_slice::<LegacyVaultData>(plaintext)
                .map(|old| !old.docker_host.is_empty() || !old.swarm_name.is_empty())
                .unwrap_or(false);
            if looks_v1 {
                migrate_v1(plaintext, path)
            } else {
                Ok((data, false))
            }
        }
        Err(_) => migrate_v1(plaintext, path),
    }
}

/// Convert a v1 payload into v2.
fn migrate_v1(plaintext: &[u8], path: &Path) -> Result<(VaultData, bool)> {
    let legacy: LegacyVaultData = serde_json::from_slice(plaintext).with_context(|| {
        format!(
            "Vault at {} is neither v{} nor a v1 document; refusing to guess",
            path.display(),
            VAULT_VERSION
        )
    })?;

    match VaultData::from_v1(&legacy) {
        Some(data) => Ok((data, true)),
        // An empty v1 vault is simply an empty v2 vault.
        None => Ok((
            VaultData {
                created_at: legacy.created_at.clone(),
                endpoints: Default::default(),
            },
            true,
        )),
    }
}

fn epoch_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault_at(tmp: &Path, password: &str) -> LocalVault {
        LocalVault::create_at(&LocalVault::at(tmp), password).unwrap()
    }

    /// A vault holding one endpoint with both tokens set.
    fn populated_vault_at(tmp: &Path, password: &str) -> LocalVault {
        let mut v = vault_at(tmp, password);
        v.store_swarm_tokens(
            "prod",
            JoinTokens {
                worker: "W-token".into(),
                manager: "M-token".into(),
            },
            Some("SWMKEY-1-abc".into()),
            "tcp://10.0.0.1:2376",
            "prod",
        )
        .unwrap();
        v
    }

    fn legacy_payload() -> Vec<u8> {
        serde_json::to_vec(&LegacyVaultData {
            created_at: "111".into(),
            swarm_name: "prod".into(),
            join_tokens: JoinTokens {
                worker: "W".into(),
                manager: "M".into(),
            },
            unlock_key: Some("SWMKEY-1-abc".into()),
            docker_host: "tcp://10.0.0.1:2376".into(),
            nodes: vec![models::NodeRecord {
                node_id: "n1".into(),
                hostname: "m1".into(),
                role: "manager".into(),
                joined_at: "1".into(),
                addr: "10.0.0.1".into(),
            }],
        })
        .unwrap()
    }

    fn write_legacy_vault(path: &Path, password: &str) {
        let (salt, nonce, ciphertext) = crypto::encrypt(&legacy_payload(), password).unwrap();
        let envelope = VaultFile {
            version: 1,
            // Absent, as vaults written before the KDF was recorded.
            kdf: None,
            salt,
            nonce,
            ciphertext,
        };
        std::fs::write(path, serde_json::to_string_pretty(&envelope).unwrap()).unwrap();
    }

    #[test]
    fn create_then_open_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        assert!(!LocalVault::exists_at(&path));

        let mut v = LocalVault::create_at(&path, "pw").unwrap();
        assert!(LocalVault::exists_at(&path));
        v.store_swarm_tokens(
            "prod",
            JoinTokens {
                worker: "w".into(),
                manager: "m".into(),
            },
            Some("k".into()),
            "tcp://h",
            "prod-swarm",
        )
        .unwrap();
        drop(v);

        let reopened = LocalVault::open_at(&path, "pw").unwrap();
        let entry = reopened.data().get("prod").unwrap();
        assert_eq!(entry.join_tokens.worker, "w");
        assert_eq!(entry.swarm_name, "prod-swarm");
    }

    #[test]
    fn create_refuses_to_clobber_an_existing_vault() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        vault_at(tmp.path(), "pw");
        let err = LocalVault::create_at(&path, "other")
            .unwrap_err()
            .to_string();
        assert!(err.contains("already exists"), "got: {err}");
    }

    #[test]
    fn wrong_password_does_not_modify_the_vault() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        vault_at(tmp.path(), "right");
        let before = std::fs::read(&path).unwrap();

        let err = LocalVault::open_at(&path, "wrong").unwrap_err().to_string();
        assert!(err.contains("Wrong password"), "got: {err}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    /// The KDF parameters are recorded in the envelope so a future change to
    /// the cost cannot silently make every existing vault undecryptable.
    #[test]
    fn kdf_parameters_are_recorded_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        populated_vault_at(tmp.path(), "pw");

        let raw = std::fs::read_to_string(&path).unwrap();
        let envelope: VaultFile = serde_json::from_str(&raw).unwrap();
        let kdf = envelope.kdf.expect("kdf params must be recorded");

        assert_eq!(kdf.algorithm, "argon2id");
        assert_eq!(kdf.output_len, 32);
        // Argon2's defaults, recorded rather than assumed.
        assert_eq!(kdf.memory_kib, argon2::Params::DEFAULT_M_COST);
        assert_eq!(kdf.iterations, argon2::Params::DEFAULT_T_COST);
        assert_eq!(kdf.parallelism, argon2::Params::DEFAULT_P_COST);
    }

    /// A vault written without the recorded KDF must still open, using the
    /// current defaults — otherwise every pre-existing vault would be lost.
    #[test]
    fn a_vault_without_recorded_kdf_still_opens() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        populated_vault_at(tmp.path(), "pw");

        // Strip the field, as vaults written before it existed would look.
        let raw = std::fs::read_to_string(&path).unwrap();
        let mut envelope: VaultFile = serde_json::from_str(&raw).unwrap();
        envelope.kdf = None;
        std::fs::write(&path, serde_json::to_string_pretty(&envelope).unwrap()).unwrap();

        let v = LocalVault::open_at(&path, "pw").expect("must still open");
        assert!(v.data().has_any());
    }

    /// A vault written with *different* KDF parameters must still open, since
    /// the recorded values — not the current defaults — are used to derive.
    #[test]
    fn non_default_kdf_parameters_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        populated_vault_at(tmp.path(), "pw");

        let raw = std::fs::read_to_string(&path).unwrap();
        let envelope: VaultFile = serde_json::from_str(&raw).unwrap();

        // Re-encrypt with cheaper parameters, and record them.
        let cheap = KdfParams {
            memory_kib: 64,
            iterations: 1,
            parallelism: 1,
            ..KdfParams::current()
        };
        let plaintext = crypto::decrypt_with(
            envelope.kdf.as_ref().unwrap(),
            &envelope.salt,
            &envelope.nonce,
            &envelope.ciphertext,
            "pw",
        )
        .unwrap();
        let (salt, nonce, ciphertext) = crypto::encrypt_with(&cheap, &plaintext, "pw").unwrap();
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&VaultFile {
                version: VAULT_VERSION,
                kdf: Some(cheap.clone()),
                salt,
                nonce,
                ciphertext,
            })
            .unwrap(),
        )
        .unwrap();

        let v = LocalVault::open_at(&path, "pw").expect("must open with its own params");
        assert_eq!(
            v.data().get("prod").map(|e| e.join_tokens.worker.clone()),
            Some("W-token".to_string()),
            "the cheaper-parameter vault must decrypt to the same content"
        );
        assert_eq!(cheap.memory_kib, 64);
    }

    /// A vault from a future swarmctl must be refused, not silently
    /// reinterpreted — the same treatment `Registry::load_from` gives.
    #[test]
    fn a_newer_format_version_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        vault_at(tmp.path(), "pw");

        let raw = std::fs::read_to_string(&path).unwrap();
        let mut envelope: VaultFile = serde_json::from_str(&raw).unwrap();
        envelope.version = VAULT_VERSION + 1;
        std::fs::write(&path, serde_json::to_string_pretty(&envelope).unwrap()).unwrap();

        let err = LocalVault::open_at(&path, "pw")
            .expect_err("a newer vault must be refused, not reinterpreted")
            .to_string();
        assert!(err.contains("newer swarmctl"), "got: {err}");
    }

    /// An unknown KDF algorithm must be reported as such rather than surfacing
    /// as a generic "wrong password".
    #[test]
    fn an_unsupported_kdf_algorithm_is_named() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        populated_vault_at(tmp.path(), "pw");

        let raw = std::fs::read_to_string(&path).unwrap();
        let mut envelope: VaultFile = serde_json::from_str(&raw).unwrap();
        envelope.kdf = Some(KdfParams {
            algorithm: "scrypt".into(),
            ..KdfParams::current()
        });
        std::fs::write(&path, serde_json::to_string_pretty(&envelope).unwrap()).unwrap();

        let err = LocalVault::open_at(&path, "pw")
            .expect_err("an unknown algorithm must be refused")
            .to_string();
        assert!(
            err.contains("key-derivation"),
            "should name the problem: {err}"
        );
    }

    /// The decrypted plaintext and the structured secrets must be scrubbed, not
    /// left in ordinary heap allocations for the process lifetime.
    #[test]
    fn decrypted_secrets_are_zeroized() {
        let mut data = VaultData::default();
        data.store(
            "prod",
            EndpointSecrets {
                join_tokens: JoinTokens {
                    worker: "swmtoken-worker-secret".into(),
                    manager: "swmtoken-manager-secret".into(),
                },
                unlock_key: Some("SWMKEY-1-secret".into()),
                ..Default::default()
            },
        );

        data.zeroize();

        assert!(
            data.endpoints.is_empty(),
            "entries must be dropped after zeroizing"
        );
    }

    #[test]
    fn opens_nothing_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let err = LocalVault::open_at(&LocalVault::at(tmp.path()), "pw")
            .unwrap_err()
            .to_string();
        assert!(err.contains("vault init"), "got: {err}");
    }

    #[test]
    fn holds_credentials_for_several_endpoints() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        let mut v = LocalVault::create_at(&path, "pw").unwrap();

        for (name, token) in [("prod-eu", "eu"), ("prod-us", "us"), ("staging", "stg")] {
            v.store_swarm_tokens(
                name,
                JoinTokens {
                    worker: format!("{}-w", token),
                    manager: format!("{}-m", token),
                },
                None,
                "ssh://h",
                name,
            )
            .unwrap();
        }
        drop(v);

        let reopened = LocalVault::open_at(&path, "pw").unwrap();
        assert_eq!(
            reopened.data().endpoint_names(),
            vec!["prod-eu", "prod-us", "staging"]
        );
        assert_eq!(
            reopened.data().get("prod-us").unwrap().join_tokens.worker,
            "us-w"
        );
        assert_eq!(reopened.status().endpoints.len(), 3);
    }

    #[test]
    fn migrates_a_v1_vault_and_rewrites_it() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        write_legacy_vault(&path, "pw");

        let v = LocalVault::open_at(&path, "pw").unwrap();
        let entry = v
            .data()
            .get("tcp---10.0.0.1-2376")
            .expect("legacy entry keyed by host");
        assert_eq!(entry.join_tokens.worker, "W");
        assert_eq!(entry.join_tokens.manager, "M");
        assert_eq!(entry.unlock_key.as_deref(), Some("SWMKEY-1-abc"));
        assert_eq!(entry.nodes.len(), 1);
        drop(v);

        // The file is now v2, so a second open needs no migration and the
        // original document shape is gone.
        let envelope: VaultFile =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(envelope.version, VAULT_VERSION);
        let again = LocalVault::open_at(&path, "pw").unwrap();
        assert_eq!(again.data().endpoint_names(), vec!["tcp---10.0.0.1-2376"]);
    }

    #[test]
    fn migrating_an_empty_v1_vault_yields_an_empty_vault() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        let (salt, nonce, ciphertext) = crypto::encrypt(
            &serde_json::to_vec(&LegacyVaultData::default()).unwrap(),
            "pw",
        )
        .unwrap();
        std::fs::write(
            &path,
            serde_json::to_string(&VaultFile {
                version: 1,
                kdf: None,
                salt,
                nonce,
                ciphertext,
            })
            .unwrap(),
        )
        .unwrap();

        let v = LocalVault::open_at(&path, "pw").unwrap();
        assert!(v.data().endpoints.is_empty());
    }

    #[test]
    fn a_v1_migration_keeps_the_old_password_working() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        write_legacy_vault(&path, "pw");
        drop(LocalVault::open_at(&path, "pw").unwrap());
        // Re-encrypted under the same password during migration.
        assert!(LocalVault::open_at(&path, "pw").is_ok());
    }

    #[test]
    fn change_password_requires_the_new_one_to_open() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        let mut v = LocalVault::create_at(&path, "old").unwrap();
        v.store_swarm_tokens(
            "prod",
            JoinTokens {
                worker: "w".into(),
                manager: String::new(),
            },
            None,
            "h",
            "prod",
        )
        .unwrap();

        v.change_password("new").unwrap();
        drop(v);

        assert!(LocalVault::open_at(&path, "old").is_err());
        let v = LocalVault::open_at(&path, "new").unwrap();
        assert_eq!(v.data().get("prod").unwrap().join_tokens.worker, "w");
    }

    #[test]
    fn rotate_tokens_keeps_other_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        let mut v = LocalVault::create_at(&path, "pw").unwrap();
        v.store_swarm_tokens(
            "prod",
            JoinTokens {
                worker: "old-w".into(),
                manager: "old-m".into(),
            },
            Some("keep-me".into()),
            "tcp://h",
            "prod-swarm",
        )
        .unwrap();

        v.rotate_tokens(
            "prod",
            JoinTokens {
                worker: "new-w".into(),
                manager: "new-m".into(),
            },
        )
        .unwrap();

        let entry = v.data().get("prod").unwrap();
        assert_eq!(entry.join_tokens.worker, "new-w");
        assert_eq!(entry.unlock_key.as_deref(), Some("keep-me"));
        assert_eq!(entry.swarm_name, "prod-swarm");
        assert_eq!(entry.docker_host, "tcp://h");
    }

    #[test]
    fn remove_endpoint_drops_only_that_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        let mut v = LocalVault::create_at(&path, "pw").unwrap();
        for name in ["a", "b"] {
            v.store_swarm_tokens(
                name,
                JoinTokens {
                    worker: "w".into(),
                    manager: String::new(),
                },
                None,
                "h",
                name,
            )
            .unwrap();
        }

        assert!(v.remove_endpoint("a").is_ok());
        assert!(v.remove_endpoint("a").is_err());
        assert_eq!(v.data().endpoint_names(), vec!["b"]);
    }

    #[test]
    fn store_rejects_an_empty_endpoint_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mut v = LocalVault::create_at(&LocalVault::at(tmp.path()), "pw").unwrap();
        assert!(
            v.store_swarm_tokens("  ", JoinTokens::default(), None, "h", "s")
                .is_err()
        );
    }

    #[test]
    fn records_nodes_without_duplicating() {
        let tmp = tempfile::tempdir().unwrap();
        let mut v = LocalVault::create_at(&LocalVault::at(tmp.path()), "pw").unwrap();
        let node = models::NodeRecord {
            node_id: "n1".into(),
            hostname: "m1".into(),
            role: "manager".into(),
            joined_at: "1".into(),
            addr: "10.0.0.1".into(),
        };
        v.record_node("prod", node.clone()).unwrap();
        v.record_node("prod", node).unwrap();
        assert_eq!(v.data().get("prod").unwrap().nodes.len(), 1);

        v.record_node(
            "prod",
            models::NodeRecord {
                node_id: "n2".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(v.data().get("prod").unwrap().nodes.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn vault_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = LocalVault::at(tmp.path());
        LocalVault::create_at(&path, "pw").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "vault holds join tokens; it must not be group/world readable"
        );
    }

    #[test]
    fn rows_summarise_each_endpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let mut v = LocalVault::create_at(&LocalVault::at(tmp.path()), "pw").unwrap();
        v.store_swarm_tokens(
            "prod",
            JoinTokens {
                worker: "w".into(),
                manager: String::new(),
            },
            None,
            "h",
            "prod-swarm",
        )
        .unwrap();
        v.data_mut().store("empty", EndpointSecrets::default());

        let rows = v.rows();
        assert_eq!(rows.len(), 2);
        let prod = rows.iter().find(|r| r.endpoint == "prod").unwrap();
        assert!(prod.has_worker_token);
        assert!(!prod.has_manager_token);
        assert_eq!(prod.swarm, "prod-swarm");
        let empty = rows.iter().find(|r| r.endpoint == "empty").unwrap();
        assert_eq!(empty.swarm, "-");
        assert!(!empty.has_worker_token);
    }

    #[test]
    fn status_counts_endpoints_with_credentials() {
        let tmp = tempfile::tempdir().unwrap();
        let mut v = LocalVault::create_at(&LocalVault::at(tmp.path()), "pw").unwrap();
        v.data_mut().store("a", EndpointSecrets::default());
        v.data_mut().store(
            "b",
            EndpointSecrets {
                join_tokens: JoinTokens {
                    worker: "w".into(),
                    manager: String::new(),
                },
                ..Default::default()
            },
        );
        let status = v.status();
        assert_eq!(status.total_endpoints, 2);
        assert_eq!(status.endpoints, vec!["b"]);
    }
}
