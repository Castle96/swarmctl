use serde::Serialize;
use tabled::Tabled;

#[derive(Clone, Tabled, Serialize)]
pub struct NetworkRow {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub internal: String,
    #[tabled(skip)]
    pub labels: String,
}

impl crate::models::RowLabels for NetworkRow {
    fn labels(&self) -> &str {
        &self.labels
    }
}

impl crate::models::Named for NetworkRow {
    fn row_name(&self) -> String {
        self.name.clone()
    }
}
