use tabled::Tabled;

#[derive(Tabled)]
pub struct TaskRow {
    pub id: String,
    pub name: String,
    pub service: String,
    pub node: String,
    pub status: String,
    pub ports: String,
}