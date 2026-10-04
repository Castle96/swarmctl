use serde::Serialize;
use tabled::Tabled;

#[derive(Clone, Tabled, Serialize)]
pub struct ConfigRow {
    pub id: String,
    pub name: String,
    pub created_at: String,
    #[tabled(skip)]
    pub labels: String,
}

impl crate::models::RowLabels for ConfigRow {
    fn labels(&self) -> &str {
        &self.labels
    }
}

impl crate::models::Named for ConfigRow {
    fn row_name(&self) -> String {
        self.name.clone()
    }
}
