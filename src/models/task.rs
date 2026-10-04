use serde::Serialize;
use tabled::Tabled;

#[derive(Clone, Tabled, Serialize)]
pub struct TaskRow {
    pub id: String,
    pub name: String,
    pub desired_state: String,
    pub current_state: String,
    pub image: String,
    pub ports: String,
    pub node: String,
    #[tabled(skip)]
    pub labels: String,
}

impl crate::models::RowLabels for TaskRow {
    fn labels(&self) -> &str {
        &self.labels
    }
}

impl crate::models::Named for TaskRow {
    fn row_name(&self) -> String {
        self.name.clone()
    }
}
