use serde::Serialize;
use tabled::Tabled;

#[derive(Tabled, Serialize)]
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
