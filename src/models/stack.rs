use serde::Serialize;
use tabled::Tabled;

#[derive(Clone, Debug, Tabled, Serialize)]
pub struct StackRow {
    pub name: String,
    pub services: String,
    pub replicas: String,
}

impl crate::models::RowLabels for StackRow {
    fn labels(&self) -> &str {
        ""
    }
}

impl crate::models::Named for StackRow {
    fn row_name(&self) -> String {
        self.name.clone()
    }
}
