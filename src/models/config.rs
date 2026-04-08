use tabled::Tabled;
use serde::Serialize;

#[derive(Tabled, Serialize)]
pub struct ConfigRow {
    pub id: String,
    pub name: String,
    pub created_at: String,
}
