pub mod crypto;
pub mod models;

use anyhow::{Context, Result};
use models::{JoinTokens, VaultData, VaultFile, VAULT_DIR, VAULT_FILE, VAULT_VERSION};
use std::fs;
use std::path::PathBuf;
use zeroize::Zeroize;

pub struct LocalVault {
    path: PathBuf,
    data: Option<VaultData>,
    password: String,
}

impl LocalVault {
    fn vault_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        PathBuf::from(home).join(VAULT_DIR)
    }

    fn vault_path() -> PathBuf {
        Self::vault_dir().join(VAULT_FILE)
    }

    pub fn exists() -> bool {
        Self::vault_path().exists()
    }

    pub fn create(password: &str) -> Result<Self> {
        let data = VaultData {
            created_at: chrono_now(),
            ..Default::default()
        };

        let plaintext = serde_json::to_vec(&data)
            .context("Failed to serialize vault data")?;

        let (salt, nonce, ciphertext) = crypto::encrypt(&plaintext, password)?;

        let vault_file = VaultFile {
            version: VAULT_VERSION,
            salt,
            nonce,
            ciphertext,
        };

        let dir = Self::vault_dir();
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create {}", dir.display()))?;

        let json = serde_json::to_string_pretty(&vault_file)?;
        fs::write(Self::vault_path(), json)
            .context("Failed to write vault file")?;

        Ok(Self {
            path: Self::vault_path(),
            data: Some(data),
            password: password.to_string(),
        })
    }

    pub fn open(password: &str) -> Result<Self> {
        let path = Self::vault_path();
        if !path.exists() {
            anyhow::bail!("Vault not found at {}. Run `swarmctl vault init` first.", path.display());
        }

        let raw = fs::read_to_string(&path)
            .context("Failed to read vault file")?;
        let vault_file: VaultFile = serde_json::from_str(&raw)
            .context("Failed to parse vault file")?;

        let plaintext = crypto::decrypt(&vault_file.salt, &vault_file.nonce, &vault_file.ciphertext, password)?;

        let data: VaultData = serde_json::from_slice(&plaintext)
            .context("Failed to decrypt vault — wrong password?")?;

        Ok(Self {
            path,
            data: Some(data),
            password: password.to_string(),
        })
    }

    pub fn save(&self) -> Result<()> {
        let data = self.data.as_ref().context("No vault data")?;

        let plaintext = serde_json::to_vec(data)
            .context("Failed to serialize vault data")?;

        let (salt, nonce, ciphertext) = crypto::encrypt(&plaintext, &self.password)?;

        let vault_file = VaultFile {
            version: VAULT_VERSION,
            salt,
            nonce,
            ciphertext,
        };

        let json = serde_json::to_string_pretty(&vault_file)?;
        fs::write(&self.path, json)
            .context("Failed to write vault file")?;

        Ok(())
    }

    pub fn data(&self) -> Option<&VaultData> {
        self.data.as_ref()
    }

    pub fn store_swarm_tokens(&mut self, tokens: JoinTokens, unlock_key: Option<String>, docker_host: &str, swarm_name: &str) -> Result<()> {
        if let Some(ref mut data) = self.data {
            data.join_tokens = tokens;
            data.unlock_key = unlock_key;
            data.docker_host = docker_host.to_string();
            data.swarm_name = swarm_name.to_string();
        }
        self.save()
    }

    pub fn rotate_tokens(&mut self, tokens: JoinTokens) -> Result<()> {
        if let Some(ref mut data) = self.data {
            data.join_tokens = tokens;
        }
        self.save()
    }

    pub fn change_password(&mut self, new_password: &str) -> Result<()> {
        self.password = new_password.to_string();
        self.save()
    }

    pub fn status(&self) -> VaultStatus {
        match &self.data {
            Some(d) => VaultStatus {
                created_at: d.created_at.clone(),
                swarm_name: d.swarm_name.clone(),
                has_tokens: !d.join_tokens.worker.is_empty() || !d.join_tokens.manager.is_empty(),
                has_unlock_key: d.unlock_key.is_some(),
                node_count: d.nodes.len(),
            },
            None => VaultStatus::default(),
        }
    }
}

impl Drop for LocalVault {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

#[derive(Default)]
pub struct VaultStatus {
    pub created_at: String,
    pub swarm_name: String,
    pub has_tokens: bool,
    pub has_unlock_key: bool,
    pub node_count: usize,
}

fn chrono_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", secs)
}
