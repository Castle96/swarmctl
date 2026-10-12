use crate::api::client::DockerClient;
use crate::cli::registry::ResourceType;
use futures::StreamExt;

/// Flags accepted by both the service and task log handlers.
#[derive(Clone, Default)]
pub struct LogOptions {
    pub follow: bool,
    pub tail: i64,
    pub previous: bool,
    pub timestamps: bool,
    pub since: Option<String>,
    pub prefix: bool,
    pub ignore_errors: bool,
}

pub async fn run(
    client: &DockerClient,
    resource: ResourceType,
    name: String,
    opts: LogOptions,
) -> anyhow::Result<()> {
    match resource {
        ResourceType::Services => logs_service(client, name, opts).await?,
        ResourceType::Tasks => logs_task(client, name, opts).await?,
        // `resolve_log_resource` rejects these before we get here.
        _ => unreachable!("logs resource narrowed to services and tasks"),
    }

    Ok(())
}

/// Resolve `--since` to a Unix timestamp.
///
/// Accepts a duration (`30s`, `5m`, `2h`, `1d`, `1w`, `1h30m`) or an absolute
/// Unix timestamp. A value with no unit is a timestamp, matching Docker.
fn parse_since(since: &str) -> anyhow::Result<i64> {
    let trimmed = since.trim();
    if let Some(seconds) = crate::utils::time::parse_duration_secs(trimmed) {
        return Ok(crate::utils::time::epoch_secs() - seconds);
    }
    // No unit: treat as an absolute timestamp.
    trimmed.parse::<i64>().map_err(|_| {
        anyhow::anyhow!(
            "Invalid --since value '{}'. Use a Unix timestamp, or a relative duration such as \
             30s, 5m, 2h, 1d, 1w, or 1h30m.",
            since
        )
    })
}

async fn logs_service(client: &DockerClient, name: String, opts: LogOptions) -> anyhow::Result<()> {
    let LogOptions {
        follow,
        tail,
        previous,
        timestamps,
        since,
        prefix,
        ignore_errors,
    } = opts;
    // Fails early with a clear message if the service does not exist.
    crate::api::service::find_service(client.inner(), &name).await?;

    if previous {
        let mut filters = std::collections::HashMap::new();
        filters.insert("service".to_string(), vec![name.clone()]);
        filters.insert(
            "desired-state".to_string(),
            vec!["shutdown".to_string(), "failed".to_string()],
        );

        let options = bollard::query_parameters::ListTasksOptions {
            filters: Some(filters),
        };
        let tasks = client.inner().list_tasks(Some(options)).await?;

        if tasks.is_empty() {
            println!("No previous tasks found for service '{}'", name);
            return Ok(());
        }

        for task in &tasks {
            let task_id = task.id.as_deref().unwrap_or("unknown");
            let task_name = task.name.as_deref().unwrap_or(task_id);
            println!("--- Previous task: {} (ID: {}) ---", task_name, task_id);

            let mut log_opts = bollard::query_parameters::LogsOptions {
                follow: false,
                stdout: true,
                stderr: true,
                tail: if tail > 0 {
                    tail.to_string()
                } else {
                    "all".to_string()
                },
                ..Default::default()
            };

            if timestamps {
                log_opts.since = since
                    .as_deref()
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(0);
            }

            let mut logs = client.inner().task_logs(task_id, Some(log_opts));
            while let Some(log_result) = logs.next().await {
                match log_result {
                    Ok(log) => {
                        let line = log.to_string();
                        let prefix_str = if prefix {
                            format!("[{}] ", task_name)
                        } else {
                            String::new()
                        };
                        print!("{}{}", prefix_str, line);
                    }
                    Err(e) => {
                        if ignore_errors {
                            eprintln!("Error reading task logs: {}", e);
                        } else {
                            return Err(e.into());
                        }
                    }
                }
            }
        }
        return Ok(());
    }

    let mut log_opts = bollard::query_parameters::LogsOptions {
        follow,
        stdout: true,
        stderr: true,
        tail: if tail > 0 {
            tail.to_string()
        } else {
            "all".to_string()
        },
        ..Default::default()
    };

    if let Some(since_str) = &since {
        let seconds = parse_since(since_str)?;
        log_opts.since = seconds as i32;
    }

    let mut logs = client.inner().service_logs(&name, Some(log_opts));

    while let Some(log_result) = logs.next().await {
        match log_result {
            Ok(log) => {
                let line = log.to_string();
                let prefix_str = if prefix {
                    format!("[{}] ", name)
                } else {
                    String::new()
                };
                print!("{}{}", prefix_str, line);
            }
            Err(e) => {
                if ignore_errors {
                    eprintln!("Error reading logs: {}", e);
                } else {
                    return Err(e.into());
                }
            }
        }
    }

    Ok(())
}

async fn logs_task(client: &DockerClient, task_id: String, opts: LogOptions) -> anyhow::Result<()> {
    let LogOptions {
        follow,
        tail,
        since,
        prefix,
        ignore_errors,
        ..
    } = opts;
    let mut log_opts = bollard::query_parameters::LogsOptions {
        follow,
        stdout: true,
        stderr: true,
        tail: if tail > 0 {
            tail.to_string()
        } else {
            "all".to_string()
        },
        ..Default::default()
    };

    if let Some(since_str) = &since {
        let seconds = parse_since(since_str)?;
        log_opts.since = seconds as i32;
    }

    let mut logs = client.inner().task_logs(&task_id, Some(log_opts));

    while let Some(log_result) = logs.next().await {
        match log_result {
            Ok(log) => {
                let line = log.to_string();
                let prefix_str = if prefix {
                    format!("[{}] ", task_id)
                } else {
                    String::new()
                };
                print!("{}{}", prefix_str, line);
            }
            Err(e) => {
                if ignore_errors {
                    eprintln!("Error reading task logs: {}", e);
                } else {
                    return Err(e.into());
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_since;
    use crate::utils::time::epoch_secs;

    /// Assert the resolved timestamp is roughly `offset` seconds ago.
    #[track_caller]
    fn assert_offset(result: anyhow::Result<i64>, offset: i64) {
        let resolved = result.expect("should parse");
        assert!(
            (epoch_secs() - resolved - offset).abs() < 5,
            "expected ~{offset}s ago, got {}",
            epoch_secs() - resolved
        );
    }

    #[test]
    fn resolves_unit_suffixes_to_relative_times() {
        assert_offset(parse_since("30s"), 30);
        assert_offset(parse_since("5m"), 300);
        assert_offset(parse_since("2h"), 7200);
        assert_offset(parse_since("1d"), 86_400);
    }

    #[test]
    fn resolves_compound_durations() {
        assert_offset(parse_since("1h30m"), 5400);
    }

    #[test]
    fn absolute_timestamps_pass_through_unchanged() {
        assert_eq!(parse_since("1754760000").unwrap(), 1_754_760_000);
        assert_eq!(parse_since("  1754760000 ").unwrap(), 1_754_760_000);
    }

    #[test]
    fn tolerates_surrounding_whitespace() {
        assert_offset(parse_since("  10m  "), 600);
    }

    #[test]
    fn rejects_unparseable_and_ambiguous_values() {
        for bad in ["abc", "10x", "m", "", "5m3", "1y"] {
            assert!(parse_since(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn rejection_message_names_the_accepted_forms() {
        let err = parse_since("nonsense").unwrap_err().to_string();
        assert!(err.contains("30s"), "got: {err}");
        assert!(err.contains("timestamp"), "got: {err}");
    }
}
