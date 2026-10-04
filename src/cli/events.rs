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
        filters: if filters.is_empty() {
            None
        } else {
            Some(filters)
        },
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
    crate::utils::time::resolve_since(s, crate::utils::time::epoch_secs())
}

#[cfg(test)]
mod tests {
    use super::normalize_time;

    #[test]
    fn relative_durations_become_timestamps() {
        for (input, expected_offset) in [("30s", 30i64), ("5m", 300), ("1h", 3600), ("1d", 86400)] {
            let resolved: i64 = normalize_time(input).parse().unwrap();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            assert!(
                (now - resolved - expected_offset).abs() < 5,
                "{input} resolved to {resolved}"
            );
        }
    }

    #[test]
    fn absolute_timestamps_pass_through() {
        assert_eq!(normalize_time("1754760000"), "1754760000");
    }

    #[test]
    fn compound_durations_are_accepted() {
        let resolved: i64 = normalize_time("1h30m").parse().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!((now - resolved - 5400).abs() < 5);
    }
}
