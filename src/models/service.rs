use tabled::Tabled;
use serde::Serialize;

#[derive(Tabled, Serialize)]
pub struct ServiceRow {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub replicas: String,
    pub image: String,
}