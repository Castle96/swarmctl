use tabled::Tabled;

#[derive(Tabled)]
pub struct NodeRow {
    pub id: String,
    pub hostname: String,
    pub status: String,
    pub availability: String,
    pub manager: String,
}