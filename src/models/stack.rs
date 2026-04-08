use tabled::Tabled;
use serde::Serialize;

#[derive(Tabled, Serialize)]
pub struct StackRow {
    pub name: String,
    pub services: String,
    pub replicas: String,
}
