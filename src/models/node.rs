use serde::Serialize;
use tabled::Tabled;

#[derive(Clone, Tabled, Serialize)]
pub struct NodeRow {
    pub id: String,
    pub hostname: String,
    pub status: String,
    pub availability: String,
    pub manager: String,
    #[tabled(skip)]
    pub labels: String,
}

impl crate::models::RowLabels for NodeRow {
    fn labels(&self) -> &str {
        &self.labels
    }
}

impl crate::models::Named for NodeRow {
    fn row_name(&self) -> String {
        self.hostname.clone()
    }
}
