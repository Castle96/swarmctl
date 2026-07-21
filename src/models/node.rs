use serde::Serialize;
use tabled::Tabled;

#[derive(Tabled, Serialize)]
pub struct NodeRow {
    pub id: String,
    pub hostname: String,
    pub status: String,
    pub availability: String,
    pub manager: String,
    #[tabled(skip)]
    pub labels: String,
}
