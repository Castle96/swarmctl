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
            let mut pairs: Vec<String> = map.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
            // HashMap iteration order is non-deterministic; sort so table output
            // and golden tests are stable across runs.
            pairs.sort();
            pairs.join(",")
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(pairs: &[(&str, &str)]) -> Option<HashMap<String, String>> {
        Some(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn selector_matches_present_key() {
        let l = labels(&[("app", "web"), ("tier", "frontend")]);
        assert!(matches_selector(&l, "app=web"));
        assert!(matches_selector(&l, "tier=frontend"));
    }

    #[test]
    fn selector_rejects_mismatch_and_missing() {
        let l = labels(&[("app", "web")]);
        assert!(!matches_selector(&l, "app=db"));
        assert!(!matches_selector(&l, "missing=x"));
    }

    #[test]
    fn selector_rejects_malformed_expression() {
        let l = labels(&[("app", "web")]);
        assert!(!matches_selector(&l, "app"));
        assert!(!matches_selector(&l, ""));
        assert!(!matches_selector(&l, "=web"));
    }

    #[test]
    fn selector_on_absent_labels_is_false() {
        assert!(!matches_selector(&None, "app=web"));
        assert!(!matches_selector(&Some(HashMap::new()), "app=web"));
    }

    #[test]
    fn selector_value_may_contain_equals() {
        let l = labels(&[("conn", "a=b")]);
        assert!(matches_selector(&l, "conn=a=b"));
    }

    #[test]
    fn format_labels_is_sorted_and_deterministic() {
        let l = labels(&[("zone", "b"), ("app", "web"), ("tier", "f")]);
        assert_eq!(format_labels(&l), "app=web,tier=f,zone=b");
        // Repeated calls must produce identical output despite HashMap ordering.
        assert_eq!(format_labels(&l), format_labels(&l));
    }

    #[test]
    fn format_labels_handles_empty_and_none() {
        assert_eq!(format_labels(&None), "");
        assert_eq!(format_labels(&Some(HashMap::new())), "");
    }
}
