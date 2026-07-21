use serde::Serialize;
use tabled::Tabled;

#[derive(Tabled, Serialize)]
pub struct ConfigRow {
    pub id: String,
    pub name: String,
    pub created_at: String,
    #[tabled(skip)]
    pub labels: String,
}
