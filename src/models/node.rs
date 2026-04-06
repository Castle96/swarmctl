use tabled::Tabled;
use serde::Serialize;

#[derive(Tabled, Serialize)]
pub struct NodeRow {
    pub id: String,
    pub hostname: String,
    pub status: String,
    pub availability: String,
    pub manager: String,
}