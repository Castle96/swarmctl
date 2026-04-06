use tabled::Tabled;

#[derive(Tabled)]
pub struct NetworkRow {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub internal: String,
}