use serde::{Deserialize, Serialize};

pub const VAULT_VERSION: u32 = 1;
pub const VAULT_DIR: &str = ".swarmctl";
pub const VAULT_FILE: &str = "vault.json";
pub const SALT_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultFile {
    pub version: u32,
    pub salt: Vec<u8>,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultData {
    pub created_at: String,
    pub swarm_name: String,
    pub join_tokens: JoinTokens,
    pub unlock_key: Option<String>,
    pub docker_host: String,
    pub nodes: Vec<NodeRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinTokens {
    pub worker: String,
    pub manager: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRecord {
    pub node_id: String,
    pub hostname: String,
    pub role: String,
    pub joined_at: String,
    pub addr: String,
}

impl Default for VaultData {
    fn default() -> Self {
        Self {
            created_at: String::new(),
            swarm_name: String::new(),
            join_tokens: JoinTokens {
                worker: String::new(),
                manager: String::new(),
            },
            unlock_key: None,
            docker_host: String::new(),
            nodes: Vec::new(),
        }
    }
}
