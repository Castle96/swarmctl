use tabled::Tabled;
use serde::Serialize;

#[derive(Tabled, Serialize)]
pub struct SecretRow {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Tabled, Serialize)]
pub struct ConfigRow {
    pub id: String,
    pub name: String,
    pub created_at: String,
}
