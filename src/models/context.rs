use serde::Serialize;
use tabled::Tabled;

#[derive(Clone, Debug, Tabled, Serialize)]
pub struct ContextRow {
    pub name: String,
    pub description: String,
    pub host: String,
    #[tabled(display_with = "display_current")]
    pub current: bool,
}

fn display_current(current: &bool) -> String {
    if *current {
        "*".to_string()
    } else {
        String::new()
    }
}

impl crate::models::RowLabels for ContextRow {
    fn labels(&self) -> &str {
        ""
    }
}

impl crate::models::Named for ContextRow {
    fn row_name(&self) -> String {
        self.name.clone()
    }
}
