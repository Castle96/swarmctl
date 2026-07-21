use serde::Serialize;
use tabled::Tabled;

#[derive(Tabled, Serialize)]
pub struct StackRow {
    pub name: String,
    pub services: String,
    pub replicas: String,
}
