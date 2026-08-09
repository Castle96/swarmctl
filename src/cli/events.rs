use crate::api::client::DockerClient;
use futures::StreamExt;
use std::collections::HashMap;

pub async fn run(
    client: &DockerClient,
    filter_types: Vec<String>,
    since: Option<String>,
) -> anyhow::Result<()> {
    let mut filters: HashMap<String, Vec<String>> = HashMap::new();
    if !filter_types.is_empty() {
        filters.insert("type".to_string(), filter_types);
    }

    let options = bollard::query_parameters::EventsOptions {
        since: since.map(|s| normalize_time(&s)),
        until: None,
        filters: if filters.is_empty() { None } else { Some(filters) },
    };

    let mut stream = client.inner().events(Some(options));

    while let Some(event) = stream.next().await {
        match event {
            Ok(ev) => {
                let action = ev.action.unwrap_or_default();
                let type_ = ev.typ.map(|t| format!("{:?}", t)).unwrap_or_default();
                let id = ev
                    .actor
                    .as_ref()
                    .and_then(|a| a.id.clone())
                    .unwrap_or_default();
                let time = ev.time.unwrap_or(0);
                println!("[{}] {} {}: {}", time, type_, action, id);
            }
            Err(e) => {
                eprintln!("Event error: {}", e);
            }
        }
    }

    Ok(())
}

pub fn normalize_time(s: &str) -> String {
    if let Some(seconds) = parse_relative_duration(s) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        (now - seconds).to_string()
    } else {
        s.to_string()
    }
}

fn parse_relative_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut total: i64 = 0;
    let mut current = String::new();
    let mut has_unit = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            current.push(c);
        } else {
            has_unit = true;
            let n: i64 = current.parse().ok()?;
            current.clear();
            let multiplier: i64 = match c {
                's' => 1,
                'm' => 60,
                'h' => 3600,
                'd' => 86400,
                'w' => 604800,
                _ => return None,
            };
            total = total.checked_add(n.checked_mul(multiplier)?)?;
        }
    }
    if has_unit && !current.is_empty() {
        let n: i64 = current.parse().ok()?;
        total = total.checked_add(n)?;
    }
    has_unit.then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_units() {
        assert_eq!(parse_relative_duration("30s"), Some(30));
        assert_eq!(parse_relative_duration("5m"), Some(300));
        assert_eq!(parse_relative_duration("1h"), Some(3600));
        assert_eq!(parse_relative_duration("1d"), Some(86400));
        assert_eq!(parse_relative_duration("1w"), Some(604800));
    }

    #[test]
    fn parses_combined() {
        assert_eq!(parse_relative_duration("2h30m"), Some(9000));
        assert_eq!(parse_relative_duration("1h15m30s"), Some(4530));
    }

    #[test]
    fn passes_through_unix_timestamp() {
        assert_eq!(parse_relative_duration("1754760000"), None);
        assert_eq!(parse_relative_duration(""), None);
        assert_eq!(parse_relative_duration("abc"), None);
    }

    #[test]
    fn normalize_converts_duration_passes_timestamp() {
        let n = normalize_time("30s");
        let ts: i64 = n.parse().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!((now - ts - 30).abs() < 5);
        assert_eq!(normalize_time("1754760000"), "1754760000");
    }
}
