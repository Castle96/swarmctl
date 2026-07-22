use std::collections::HashMap;

pub fn matches_selector(labels: &Option<HashMap<String, String>>, selector: &str) -> bool {
    let Some((key, value)) = selector.split_once('=') else {
        return false;
    };
    labels
        .as_ref()
        .and_then(|l| l.get(key))
        .map(|v| v == value)
        .unwrap_or(false)
}

pub fn format_labels(labels: &Option<HashMap<String, String>>) -> String {
    match labels {
        Some(map) if !map.is_empty() => {
            let pairs: Vec<String> = map.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
            pairs.join(",")
        }
        _ => String::new(),
    }
}
