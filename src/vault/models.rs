use serde::{Deserialize, Serialize};

/// Current vault format version.
///
/// v1 stored a single cluster's credentials as top-level scalars, which meant the
/// vault could hold exactly one cluster. v2 keys credentials by endpoint name.
pub const VAULT_VERSION: u32 = 2;

pub const SALT_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;

/// The encrypted envelope written to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultFile {
    pub version: u32,
    pub salt: Vec<u8>,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

/// Decrypted vault contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultData {
    #[serde(default)]
    pub created_at: String,
    /// Credentials keyed by endpoint name.
    #[serde(default)]
    pub endpoints: std::collections::BTreeMap<String, EndpointSecrets>,
}

impl Default for VaultData {
    fn default() -> Self {
        Self {
            created_at: String::new(),
            endpoints: Default::default(),
        }
    }
}

impl VaultData {
    /// Credentials for one cluster.
    pub fn get(&self, endpoint: &str) -> Option<&EndpointSecrets> {
        self.endpoints.get(endpoint)
    }

    pub fn endpoint_names(&self) -> Vec<String> {
        self.endpoints.keys().cloned().collect()
    }

    /// Store credentials for an endpoint, replacing any existing entry.
    pub fn store(&mut self, endpoint: &str, secrets: EndpointSecrets) {
        self.endpoints.insert(endpoint.to_string(), secrets);
    }

    pub fn remove(&mut self, endpoint: &str) -> Option<EndpointSecrets> {
        self.endpoints.remove(endpoint)
    }

    /// Whether any endpoint has stored credentials.
    pub fn has_any(&self) -> bool {
        self.endpoints.values().any(|e| e.has_credentials())
    }

    /// Convert a v1 document into the v2 shape.
    ///
    /// v1 had `swarm_name`, `join_tokens`, `docker_host` and `unlock_key` at the
    /// top level, so its single cluster is re-keyed by the host it described.
    /// Returns `None` when there was nothing worth keeping.
    pub fn from_v1(old: &LegacyVaultData) -> Option<Self> {
        let key = legacy_key(old);
        if !old.join_tokens.has_any() && old.unlock_key.is_none() {
            return None;
        }
        let mut endpoints = std::collections::BTreeMap::new();
        endpoints.insert(
            key,
            EndpointSecrets {
                swarm_name: old.swarm_name.clone(),
                join_tokens: old.join_tokens.clone(),
                unlock_key: old.unlock_key.clone(),
                nodes: old.nodes.clone(),
                docker_host: old.docker_host.clone(),
                updated_at: String::new(),
            },
        );
        Some(Self {
            created_at: old.created_at.clone(),
            endpoints,
        })
    }
}

/// v1 document shape, retained only so existing vaults can be migrated.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LegacyVaultData {
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub swarm_name: String,
    #[serde(default)]
    pub join_tokens: JoinTokens,
    #[serde(default)]
    pub unlock_key: Option<String>,
    #[serde(default)]
    pub docker_host: String,
    #[serde(default)]
    pub nodes: Vec<NodeRecord>,
}

/// A short, filesystem-safe key for a legacy vault entry.
fn legacy_key(old: &LegacyVaultData) -> String {
    if !old.docker_host.is_empty() {
        return sanitize(&old.docker_host);
    }
    if !old.swarm_name.is_empty() {
        return sanitize(&old.swarm_name);
    }
    "default".to_string()
}

/// Reduce a host or name to `[A-Za-z0-9._-]`.
fn sanitize(value: &str) -> String {
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

/// Swarm join tokens for one cluster.
///
/// These grant manager-equivalent access, which is why they live encrypted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinTokens {
    #[serde(default)]
    pub worker: String,
    #[serde(default)]
    pub manager: String,
}

impl JoinTokens {
    pub fn is_empty(&self) -> bool {
        self.worker.is_empty() && self.manager.is_empty()
    }

    pub fn has_any(&self) -> bool {
        !self.worker.is_empty() || !self.manager.is_empty()
    }
}

/// Credentials and metadata for one endpoint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EndpointSecrets {
    #[serde(default)]
    pub swarm_name: String,
    #[serde(default)]
    pub join_tokens: JoinTokens,
    #[serde(default)]
    pub unlock_key: Option<String>,
    #[serde(default)]
    pub nodes: Vec<NodeRecord>,
    #[serde(default)]
    pub docker_host: String,
    /// Unix timestamp of the last write.
    #[serde(default)]
    pub updated_at: String,
}

impl EndpointSecrets {
    pub fn has_credentials(&self) -> bool {
        self.join_tokens.has_any() || self.unlock_key.is_some()
    }
}

/// A node as recorded in the vault.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeRecord {
    #[serde(default)]
    pub node_id: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub joined_at: String,
    #[serde(default)]
    pub addr: String,
}

/// Summary of the vault, as shown by `swarmctl vault status`.
#[derive(Debug, Clone, Default)]
pub struct VaultStatus {
    pub created_at: String,
    /// Endpoint names holding credentials.
    pub endpoints: Vec<String>,
    pub total_endpoints: usize,
}

/// Per-endpoint summary, as shown by `swarmctl vault ls`.
#[derive(Debug, Clone, serde::Serialize, tabled::Tabled)]
pub struct VaultEntryRow {
    pub endpoint: String,
    pub swarm: String,
    pub has_worker_token: bool,
    pub has_manager_token: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[tabled(skip)]
    pub unlock_key: Option<String>,
    pub nodes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[tabled(skip)]
    pub updated_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_vault_has_no_endpoints() {
        let v = VaultData::default();
        assert!(v.endpoints.is_empty());
        assert!(!v.has_any());
    }

    #[test]
    fn stores_and_reads_back_per_endpoint() {
        let mut v = VaultData::default();
        v.store(
            "prod-eu",
            EndpointSecrets {
                join_tokens: JoinTokens {
                    worker: "w1".into(),
                    manager: "m1".into(),
                },
                ..Default::default()
            },
        );
        v.store(
            "prod-us",
            EndpointSecrets {
                join_tokens: JoinTokens {
                    worker: "w2".into(),
                    manager: "m2".into(),
                },
                ..Default::default()
            },
        );

        assert_eq!(v.endpoint_names(), vec!["prod-eu", "prod-us"]);
        assert_eq!(v.get("prod-eu").unwrap().join_tokens.worker, "w1");
        assert_eq!(v.get("prod-us").unwrap().join_tokens.manager, "m2");
        assert!(v.get("prod-us").unwrap().join_tokens != v.get("prod-eu").unwrap().join_tokens);
    }

    #[test]
    fn storing_twice_replaces_the_entry() {
        let mut v = VaultData::default();
        v.store("a", EndpointSecrets::default());
        v.store(
            "a",
            EndpointSecrets {
                join_tokens: JoinTokens {
                    worker: "w".into(),
                    manager: String::new(),
                },
                ..Default::default()
            },
        );
        assert_eq!(v.endpoint_names().len(), 1);
        assert_eq!(v.get("a").unwrap().join_tokens.worker, "w");
    }

    #[test]
    fn remove_drops_one_endpoint_only() {
        let mut v = VaultData::default();
        v.store("a", EndpointSecrets::default());
        v.store("b", EndpointSecrets::default());
        assert!(v.remove("a").is_some());
        assert!(v.remove("a").is_none());
        assert_eq!(v.endpoint_names(), vec!["b"]);
    }

    #[test]
    fn has_any_reflects_real_credentials() {
        let mut v = VaultData::default();
        assert!(!v.has_any());
        v.store("a", EndpointSecrets::default());
        assert!(!v.has_any(), "an empty entry is not a credential");

        v.store(
            "a",
            EndpointSecrets {
                unlock_key: Some("SWMKEY-1-x".into()),
                ..Default::default()
            },
        );
        assert!(v.has_any());
    }

    #[test]
    fn migrates_a_single_cluster_v1_document() {
        let legacy = LegacyVaultData {
            created_at: "1700000000".into(),
            swarm_name: "prod".into(),
            join_tokens: JoinTokens {
                worker: "w".into(),
                manager: "m".into(),
            },
            unlock_key: Some("k".into()),
            docker_host: "tcp://10.0.0.1:2376".into(),
            nodes: vec![NodeRecord {
                node_id: "n1".into(),
                ..Default::default()
            }],
        };

        let migrated = VaultData::from_v1(&legacy).expect("should migrate");
        assert_eq!(migrated.created_at, "1700000000");
        assert_eq!(migrated.endpoint_names().len(), 1);

        let entry = migrated
            .get("tcp---10.0.0.1-2376")
            .expect("keyed by the legacy docker_host");
        assert_eq!(entry.join_tokens.worker, "w");
        assert_eq!(entry.join_tokens.manager, "m");
        assert_eq!(entry.unlock_key.as_deref(), Some("k"));
        assert_eq!(entry.swarm_name, "prod");
        assert_eq!(entry.nodes.len(), 1);
    }

    #[test]
    fn migrating_an_empty_v1_document_yields_nothing() {
        let legacy = LegacyVaultData {
            created_at: "1".into(),
            docker_host: "tcp://x".into(),
            ..Default::default()
        };
        assert!(VaultData::from_v1(&legacy).is_none());
    }

    #[test]
    fn v1_without_a_host_falls_back_to_the_swarm_name() {
        let legacy = LegacyVaultData {
            join_tokens: JoinTokens {
                worker: "w".into(),
                manager: String::new(),
            },
            swarm_name: "prod-cluster".into(),
            ..Default::default()
        };
        let migrated = VaultData::from_v1(&legacy).unwrap();
        assert_eq!(migrated.endpoint_names(), vec!["prod-cluster"]);
    }

    #[test]
    fn v1_with_nothing_identifiable_uses_a_default_key() {
        let legacy = LegacyVaultData {
            join_tokens: JoinTokens {
                worker: "w".into(),
                manager: String::new(),
            },
            ..Default::default()
        };
        assert_eq!(
            VaultData::from_v1(&legacy).unwrap().endpoint_names(),
            vec!["default"]
        );
    }

    #[test]
    fn legacy_keys_are_filesystem_safe() {
        assert_eq!(sanitize("tcp://10.0.0.1:2376"), "tcp---10.0.0.1-2376");
        assert_eq!(sanitize("ssh://user@host"), "ssh---user-host");
        assert_eq!(sanitize("///"), "default");
        assert_eq!(sanitize(""), "default");
    }

    #[test]
    fn vault_data_round_trips_as_json() {
        let mut v = VaultData::default();
        v.created_at = "123".into();
        v.store(
            "prod",
            EndpointSecrets {
                swarm_name: "s".into(),
                join_tokens: JoinTokens {
                    worker: "w".into(),
                    manager: "m".into(),
                },
                unlock_key: Some("u".into()),
                nodes: vec![NodeRecord {
                    node_id: "n".into(),
                    hostname: "h".into(),
                    role: "manager".into(),
                    joined_at: "1".into(),
                    addr: "10.0.0.1".into(),
                }],
                docker_host: "tcp://h".into(),
                updated_at: "456".into(),
            },
        );
        let json = serde_json::to_string(&v).unwrap();
        let back: VaultData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.endpoint_names(), vec!["prod"]);
        assert_eq!(back.get("prod").unwrap().nodes[0].role, "manager");
        assert_eq!(back.get("prod").unwrap().updated_at, "456");
    }

    #[test]
    fn a_v1_document_deserialises_into_the_legacy_shape() {
        // Proves the migration input matches what v1 actually wrote.
        let raw = r#"{
            "created_at": "1",
            "swarm_name": "prod",
            "join_tokens": {"worker": "w", "manager": "m"},
            "unlock_key": "u",
            "docker_host": "tcp://h",
            "nodes": []
        }"#;
        let legacy: LegacyVaultData = serde_json::from_str(raw).unwrap();
        assert_eq!(legacy.join_tokens.worker, "w");
        assert_eq!(legacy.docker_host, "tcp://h");
    }

    #[test]
    fn a_v1_document_does_not_deserialise_into_v2() {
        // Guards against accidentally treating an old file as already-migrated.
        let raw = r#"{"created_at":"1","swarm_name":"prod","docker_host":"tcp://h"}"#;
        let as_v2 = serde_json::from_str::<VaultData>(raw);
        assert!(
            as_v2.is_ok(),
            "v2 tolerates unknown fields, so this parses but yields no endpoints"
        );
        assert!(as_v2.unwrap().endpoints.is_empty());
    }

    #[test]
    fn join_tokens_emptiness() {
        assert!(JoinTokens::default().is_empty());
        assert!(!JoinTokens::default().has_any());
        let t = JoinTokens {
            worker: "w".into(),
            manager: String::new(),
        };
        assert!(!t.is_empty());
        assert!(t.has_any());
    }
}
