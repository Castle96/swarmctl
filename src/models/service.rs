use serde::Serialize;
use tabled::Tabled;

#[derive(Tabled, Serialize)]
pub struct ServiceRow {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub replicas: String,
    pub image: String,
    #[tabled(skip)]
    pub labels: String,
}
