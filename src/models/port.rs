use serde::Serialize;
use std::hash::{Hash, Hasher};
use tabled::Tabled;

#[derive(Tabled, Serialize, Clone)]
pub struct PortRow {
    pub port: String,
    pub protocol: String,
    pub service: String,
    pub target_port: String,
    pub publish_mode: String,
}

impl PartialEq for PortRow {
    fn eq(&self, other: &Self) -> bool {
        self.port == other.port && self.protocol == other.protocol && 
        self.service == other.service && self.target_port == other.target_port && 
        self.publish_mode == other.publish_mode
    }
}

impl Eq for PortRow {}

impl Hash for PortRow {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.port.hash(state);
        self.protocol.hash(state);
        self.service.hash(state);
        self.target_port.hash(state);
        self.publish_mode.hash(state);
    }
}

#[derive(Tabled, Serialize, Clone)]
pub struct ServicePortInfo {
    pub service_name: String,
    pub published_port: String,
    pub target_port: String,
    pub protocol: String,
    pub publish_mode: String,
}
