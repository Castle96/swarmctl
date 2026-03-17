use tabled::Tabled;

#[derive(Tabled)]
pub struct ServiceRow {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub replicas: String,
    pub image: String,
}