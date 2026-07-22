use serde::Serialize;
use tabled::Tabled;

#[derive(Tabled, Serialize)]
pub struct VolumeRow {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub labels: String,
    pub scope: String,
    #[tabled(skip)]
    pub created_at: String,
}
