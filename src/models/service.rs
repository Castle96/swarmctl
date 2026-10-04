use serde::Serialize;
use tabled::Tabled;

#[derive(Clone, Debug, Tabled, Serialize)]
pub struct ServiceRow {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub replicas: String,
    pub image: String,
    #[tabled(skip)]
    pub labels: String,
}

impl crate::models::RowLabels for ServiceRow {
    fn labels(&self) -> &str {
        &self.labels
    }
}

impl crate::models::Named for ServiceRow {
    fn row_name(&self) -> String {
        self.name.clone()
    }
}
