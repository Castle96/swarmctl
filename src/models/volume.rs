use serde::Serialize;
use tabled::Tabled;

#[derive(Clone, Debug, Tabled, Serialize)]
pub struct VolumeRow {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub labels: String,
    pub scope: String,
    #[tabled(skip)]
    pub created_at: String,
}

impl crate::models::RowLabels for VolumeRow {
    fn labels(&self) -> &str {
        &self.labels
    }
}

impl crate::models::Named for VolumeRow {
    fn row_name(&self) -> String {
        self.name.clone()
    }
}
