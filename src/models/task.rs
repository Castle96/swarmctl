use tabled::Tabled;
use serde::Serialize;

#[derive(Tabled, Serialize)]
pub struct TaskRow {
    pub id: String,
    pub name: String,
    pub desired_state: String,
    pub current_state: String,
    pub image: String,
    pub ports: String,
    pub node: String,
}
