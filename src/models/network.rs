use serde::Serialize;
use tabled::Tabled;

#[derive(Tabled, Serialize)]
pub struct NetworkRow {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub internal: String,
    #[tabled(skip)]
    pub labels: String,
}
