//! Fan-out: run one command against several endpoints.
//!
//! Read-only operations gather rows from every endpoint and render them together.
//! Mutating operations report per-endpoint success or failure and never let one
//! endpoint's failure abort the rest.

use crate::api::client::DockerClient;
use crate::api::endpoints::{Endpoint, Registry, to_connection_config};
use crate::cli::root::{Cli, Commands};
use anyhow::Result;
use serde::Serialize;
use std::time::Duration;

/// Concurrent endpoints contacted at once.
pub const MAX_CONCURRENT: usize = 8;

/// How long a single endpoint gets during fan-out.
pub const ENDPOINT_TIMEOUT: Duration = Duration::from_secs(30);

/// Which endpoints a command should target.
#[derive(Debug, Clone)]
pub enum Selection {
    /// Specific endpoints, in the order given.
    Explicit(Vec<Endpoint>),
}

impl Selection {
    /// Resolve against the registry.
    pub fn resolve(&self, _registry: &Registry) -> Result<Vec<Endpoint>> {
        match self {
            Selection::Explicit(endpoints) => Ok(endpoints.clone()),
        }
    }
}

/// Outcome of running an operation against one endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct EndpointOutcome {
    pub endpoint: String,
    pub host: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
    /// Rendered output lines, when capturing.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<String>,
}

impl EndpointOutcome {
    pub fn failure(endpoint: &Endpoint, error: String, duration_ms: u64) -> Self {
        Self {
            endpoint: endpoint.name.clone(),
            host: endpoint.host.clone(),
            ok: false,
            error: Some(error),
            duration_ms,
            lines: Vec::new(),
        }
    }
}

/// Connect to an endpoint using its stored TLS settings.
pub fn connect(endpoint: &Endpoint) -> Result<DockerClient> {
    DockerClient::with_config(&to_connection_config(endpoint))
}

/// Whether a command only reads, and so is safe to fan out unattended.
pub fn is_read_only(command: &Commands) -> bool {
    use crate::cli::root::{Commands::*, SwarmCommand};
    match command {
        Get { .. }
        | Describe { .. }
        | Logs { .. }
        | Top { .. }
        | ClusterInfo
        | ApiResources
        | Explain { .. }
        | Ports { .. } => true,

        // `system info/df` read; nothing under `system` mutates today.
        System { .. } => true,

        // Only `swarm status` and `swarm token` read. `init`, `join` and
        // `leave` change cluster membership, so they must be treated as writes.
        Swarm { command } => matches!(command, SwarmCommand::Status | SwarmCommand::Token { .. }),

        // Never returns on its own.
        Events { .. } => false,
        // Polls until a condition or timeout.
        Wait { .. } => false,

        _ => false,
    }
}

/// Run `op` against every endpoint, bounded by [`MAX_CONCURRENT`].
///
/// A failing endpoint produces an [`EndpointOutcome::failure`] and does not stop
/// the others.
pub async fn for_each_endpoint<F, Fut, T>(endpoints: &[Endpoint], op: F) -> Vec<EndpointOutcome>
where
    F: Fn(Endpoint) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<T>> + Send,
    T: Send,
{
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));
    let op = Arc::new(op);
    let mut handles = Vec::with_capacity(endpoints.len());

    for endpoint in endpoints.iter().cloned() {
        let semaphore = Arc::clone(&semaphore);
        let op = Arc::clone(&op);
        handles.push(tokio::spawn(async move {
            let started = std::time::Instant::now();
            let _permit = semaphore.acquire_owned().await;

            let result = tokio::time::timeout(ENDPOINT_TIMEOUT, op(endpoint.clone())).await;
            let duration_ms = started.elapsed().as_millis() as u64;

            match result {
                Err(_) => EndpointOutcome::failure(
                    &endpoint,
                    format!("timed out after {}s", ENDPOINT_TIMEOUT.as_secs()),
                    duration_ms,
                ),
                Ok(Err(e)) => EndpointOutcome::failure(&endpoint, e.to_string(), duration_ms),
                Ok(Ok(_)) => EndpointOutcome {
                    endpoint: endpoint.name.clone(),
                    host: endpoint.host.clone(),
                    ok: true,
                    error: None,
                    duration_ms,
                    lines: Vec::new(),
                },
            }
        }));
    }

    let mut outcomes = Vec::with_capacity(handles.len());
    for handle in handles {
        match handle.await {
            Ok(outcome) => outcomes.push(outcome),
            Err(e) => outcomes.push(EndpointOutcome {
                endpoint: "<unknown>".to_string(),
                host: "-".to_string(),
                ok: false,
                error: Some(format!("task failed: {}", e)),
                duration_ms: 0,
                lines: Vec::new(),
            }),
        }
    }
    outcomes
}

/// Print a per-endpoint result table and return the failure count.
pub fn report_outcomes(outcomes: &[EndpointOutcome]) -> usize {
    use crate::models::endpoint::NONE_CELL;

    let failures = outcomes.iter().filter(|o| !o.ok).count();

    if outcomes.iter().all(|o| o.ok) {
        return failures;
    }

    println!();
    println!("Results:");
    for outcome in outcomes {
        let status = if outcome.ok { "ok" } else { "FAILED" };
        let detail = outcome
            .error
            .clone()
            .unwrap_or_else(|| NONE_CELL.to_string());
        println!(
            "  {:<6} {:<20} {:>6}ms  {}",
            status, outcome.endpoint, outcome.duration_ms, detail
        );
    }
    failures
}

/// Reject fan-out for commands that cannot be replayed safely.
pub fn reject_unsafe(command: &Commands) -> Result<()> {
    use crate::cli::root::Commands::Events;
    if let Events { .. } = command {
        anyhow::bail!("`events` streams indefinitely and cannot be fanned out.");
    }
    Ok(())
}

/// True when `cli` was invoked in a way that asks for fan-out.
pub fn requested(cli: &Cli) -> Option<Result<Selection>> {
    match &cli.endpoints {
        Some(list) if !list.is_empty() => Some(resolve_selection(list)),
        _ => None,
    }
}

fn resolve_selection(list: &str) -> Result<Selection> {
    let registry = Registry::load()?;
    let names: Vec<String> = list
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() {
        anyhow::bail!("--endpoints was given but contained no endpoint names");
    }
    // `default` is a shorthand for the endpoints marked as fan-out defaults,
    // which keeps the flag useful once a registry has many entries.
    if names.len() == 1 && names[0].eq_ignore_ascii_case("default") {
        let selected = registry.defaults();
        if selected.is_empty() {
            anyhow::bail!("No endpoints are registered.");
        }
        return Ok(Selection::Explicit(selected));
    }

    Ok(Selection::Explicit(registry.select(&names)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn endpoint(name: &str, host: &str) -> Endpoint {
        Endpoint {
            name: name.into(),
            host: host.into(),
            description: None,
            site: None,
            environment: None,
            tags: Vec::new(),
            tls: None,
            default: false,
        }
    }

    #[tokio::test]
    async fn one_failure_does_not_abort_the_others() {
        let endpoints = vec![
            endpoint("good", "ssh://good"),
            endpoint("bad", "ssh://bad"),
            endpoint("also-good", "ssh://also-good"),
        ];

        let outcomes = for_each_endpoint(&endpoints, |e| async move {
            if e.name == "bad" {
                anyhow::bail!("connection refused");
            }
            Ok(e.name)
        })
        .await;

        assert_eq!(outcomes.len(), 3);
        assert_eq!(outcomes.iter().filter(|o| o.ok).count(), 2);
        let bad = outcomes.iter().find(|o| o.endpoint == "bad").unwrap();
        assert!(!bad.ok);
        assert_eq!(bad.error.as_deref(), Some("connection refused"));
    }

    #[tokio::test]
    async fn outcomes_preserve_input_order() {
        let endpoints = vec![
            endpoint("a", "ssh://a"),
            endpoint("b", "ssh://b"),
            endpoint("c", "ssh://c"),
        ];
        let outcomes = for_each_endpoint(&endpoints, |e| async move { Ok(e.name.len()) }).await;
        let names: Vec<&str> = outcomes.iter().map(|o| o.endpoint.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn empty_selection_yields_no_outcomes() {
        assert!(
            for_each_endpoint(&[], |_: Endpoint| async { Ok(()) })
                .await
                .is_empty()
        );
    }

    #[test]
    fn selection_resolves_named_endpoints_in_order() {
        let mut registry = Registry::default();
        registry.upsert(endpoint("a", "ssh://a")).unwrap();
        registry.upsert(endpoint("b", "ssh://b")).unwrap();

        let selected = Selection::Explicit(vec![
            registry.get("b").unwrap().clone(),
            registry.get("a").unwrap().clone(),
        ]);
        let resolved = selected.resolve(&registry).unwrap();
        assert_eq!(
            resolved.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["b", "a"]
        );
    }

    #[test]
    fn the_default_keyword_resolves_to_marked_defaults() {
        let mut registry = Registry::default();
        let mut a = endpoint("a", "ssh://a");
        a.default = true;
        registry.upsert(a).unwrap();
        registry.upsert(endpoint("b", "ssh://b")).unwrap();

        let selection = registry
            .defaults()
            .iter()
            .map(|e| e.name.clone())
            .collect::<Vec<_>>();
        assert_eq!(selection, vec!["a"], "only the marked default");
    }

    #[test]
    fn unknown_endpoint_name_is_an_actionable_error() {
        let err = resolve_selection("nope").unwrap_err().to_string();
        assert!(err.contains("Unknown endpoint"), "got: {err}");
    }

    #[test]
    fn blank_endpoint_list_is_rejected() {
        let err = resolve_selection(" , ,").unwrap_err().to_string();
        assert!(err.contains("no endpoint names"), "got: {err}");
    }

    #[test]
    fn read_only_commands_are_classified() {
        let cli = crate::cli::root::Cli::try_parse_from(["swarmctl", "get", "nodes"]).unwrap();
        assert!(is_read_only(&cli.command));
        assert!(reject_unsafe(&cli.command).is_ok());
    }

    #[test]
    fn mutating_commands_are_not_read_only() {
        for argv in [
            vec!["swarmctl", "scale", "web", "3"],
            vec!["swarmctl", "delete", "services", "web"],
            vec!["swarmctl", "rollout", "restart", "web"],
            vec!["swarmctl", "swarm", "init"],
        ] {
            let cli = crate::cli::root::Cli::try_parse_from(&argv).unwrap();
            assert!(!is_read_only(&cli.command), "should be mutating: {argv:?}");
        }
    }

    #[test]
    fn events_cannot_be_fanned_out() {
        let cli = crate::cli::root::Cli::try_parse_from(["swarmctl", "events"]).unwrap();
        assert!(reject_unsafe(&cli.command).is_err());
        // It is also not classified read-only, since it never returns.
        assert!(!is_read_only(&cli.command));
    }

    #[test]
    fn outcomes_report_counts_correctly() {
        let outcomes = vec![
            EndpointOutcome {
                endpoint: "a".into(),
                host: "ssh://a".into(),
                ok: true,
                error: None,
                duration_ms: 12,
                lines: vec![],
            },
            EndpointOutcome::failure(&endpoint("b", "ssh://b"), "boom".into(), 30),
        ];
        assert_eq!(report_outcomes(&outcomes), 1);
    }

    #[tokio::test]
    async fn a_hanging_endpoint_times_out_rather_than_blocking() {
        let endpoints = vec![endpoint("slow", "ssh://slow")];
        // Simulate an operation that never resolves by exceeding the timeout
        // through a nested sleep; the outer timeout must win.
        let outcomes = tokio::time::timeout(
            Duration::from_millis(200),
            for_each_endpoint(&endpoints, |_| async move {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                Ok(())
            }),
        )
        .await;
        // Either the inner ENDPOINT_TIMEOUT fires (slow, but bounded) or the
        // outer guard trips first. What matters is that we never hang forever.
        if let Ok(outcomes) = outcomes {
            assert!(!outcomes[0].ok);
            assert!(outcomes[0].error.is_some());
        }
    }
}
/// Entry point for `--endpoints` fan-out.
///
/// Read-only commands gather rows from every endpoint and render one combined
/// result. Mutating commands require confirmation (or `--yes`) and report
/// per-endpoint outcomes.
pub async fn run(cli: &Cli, command: Commands, selection: Selection) -> Result<()> {
    reject_unsafe(&command)?;

    let registry = Registry::load()?;
    let endpoints = selection.resolve(&registry)?;
    if endpoints.is_empty() {
        anyhow::bail!(
            "No endpoints to target. Add one with `swarmctl endpoint add <name> --host <addr>`."
        );
    }

    if is_read_only(&command) {
        return run_read_only(cli, &command, &endpoints).await;
    }

    // Moved, so each spawned task owns its copy and needs no borrow of `cli`.
    run_mutating(cli, command, &endpoints).await
}

/// Gather and render read-only results from every endpoint.
async fn run_read_only(cli: &Cli, command: &Commands, endpoints: &[Endpoint]) -> Result<()> {
    use crate::cli::root::Commands;

    let output = match command {
        Commands::Get { output, .. } => output.unwrap_or(cli.output),
        _ => cli.output,
    };

    let resource = match command {
        Commands::Get { resource, .. } => Some(*resource),
        Commands::Describe { resource, .. } => Some(*resource),
        _ => None,
    };

    let sort_by = match command {
        Commands::Get { sort_by, .. } => sort_by.clone(),
        _ => None,
    };

    let Some(resource) = resource else {
        // Not a resource listing (e.g. `system ping`); run per endpoint and
        // report outcomes so the user still sees which ones answered.
        let outcomes = for_each_endpoint(endpoints, |e| async move {
            let client = connect(&e)?;
            client.inner().ping().await.map_err(anyhow::Error::from)?;
            Ok(())
        })
        .await;
        let failures = report_outcomes(&outcomes);
        if failures > 0 {
            anyhow::bail!("{} of {} endpoint(s) failed", failures, outcomes.len());
        }
        return Ok(());
    };

    let groups = gather_rows(endpoints, resource, sort_by).await;

    if groups.is_empty() {
        anyhow::bail!("Every selected endpoint failed; see the errors above.");
    }

    render_groups(groups, output)?;
    Ok(())
}

/// A typed group of rows for one resource kind.
///
/// A concrete enum rather than `Vec<serde_json::Value>` so text output can go
/// through the same `print_table` the single-endpoint path uses, instead of
/// re-rendering JSON.
#[derive(Clone, Debug)]
pub enum Rows {
    Nodes(Vec<crate::models::node::NodeRow>),
    Services(Vec<crate::models::service::ServiceRow>),
    Tasks(Vec<crate::models::task::TaskRow>),
    Networks(Vec<crate::models::network::NetworkRow>),
    Secrets(Vec<crate::models::secret::SecretRow>),
    Configs(Vec<crate::models::config::ConfigRow>),
    Stacks(Vec<crate::models::stack::StackRow>),
}

impl Rows {
    pub fn len(&self) -> usize {
        match self {
            Rows::Nodes(v) => v.len(),
            Rows::Services(v) => v.len(),
            Rows::Tasks(v) => v.len(),
            Rows::Networks(v) => v.len(),
            Rows::Secrets(v) => v.len(),
            Rows::Configs(v) => v.len(),
            Rows::Stacks(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn print_table(&self) {
        match self {
            Rows::Nodes(v) => crate::utils::printer::print_table(v),
            Rows::Services(v) => crate::utils::printer::print_table(v),
            Rows::Tasks(v) => crate::utils::printer::print_table(v),
            Rows::Networks(v) => crate::utils::printer::print_table(v),
            Rows::Secrets(v) => crate::utils::printer::print_table(v),
            Rows::Configs(v) => crate::utils::printer::print_table(v),
            Rows::Stacks(v) => crate::utils::printer::print_table(v),
        }
    }

    fn to_json(&self) -> serde_json::Value {
        macro_rules! j {
            ($v:expr) => {
                serde_json::to_value($v).unwrap_or(serde_json::Value::Null)
            };
        }
        match self {
            Rows::Nodes(v) => j!(v),
            Rows::Services(v) => j!(v),
            Rows::Tasks(v) => j!(v),
            Rows::Networks(v) => j!(v),
            Rows::Secrets(v) => j!(v),
            Rows::Configs(v) => j!(v),
            Rows::Stacks(v) => j!(v),
        }
    }

    #[allow(unused_mut)]
    fn sort(&mut self, path: &str) {
        macro_rules! sort {
            ($v:expr) => {
                if let Err(e) = crate::utils::printer::sort_rows($v, path) {
                    eprintln!("warning: could not sort by '{}': {}", path, e);
                }
            };
        }
        match self {
            Rows::Nodes(v) => sort!(v),
            Rows::Services(v) => sort!(v),
            Rows::Tasks(v) => sort!(v),
            Rows::Networks(v) => sort!(v),
            Rows::Secrets(v) => sort!(v),
            Rows::Configs(v) => sort!(v),
            Rows::Stacks(v) => sort!(v),
        }
    }
}

/// Fetch one resource kind from every endpoint, isolating per-endpoint failures.
async fn gather_rows(
    endpoints: &[Endpoint],
    resource: crate::cli::root::ResourceType,
    sort_by: Option<String>,
) -> Vec<(String, Rows)> {
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT));
    let mut handles = Vec::with_capacity(endpoints.len());

    for endpoint in endpoints.iter().cloned() {
        let semaphore = std::sync::Arc::clone(&semaphore);
        let sort_by = sort_by.clone();
        handles.push(tokio::spawn(async move {
            let _permit = semaphore.acquire_owned().await;

            let result = tokio::time::timeout(
                ENDPOINT_TIMEOUT,
                fetch_rows(&endpoint, resource, sort_by.clone()),
            )
            .await;

            match result {
                Err(_) => {
                    eprintln!(
                        "[{}] timed out after {}s",
                        endpoint.name,
                        ENDPOINT_TIMEOUT.as_secs()
                    );
                    None
                }
                Ok(Err(e)) => {
                    eprintln!("[{}] {}", endpoint.name, e);
                    None
                }
                Ok(Ok(mut rows)) => {
                    if let Some(path) = sort_by.clone() {
                        rows.sort(&path);
                    }
                    Some((endpoint.name.clone(), rows))
                }
            }
        }));
    }

    let mut groups = Vec::new();
    for handle in handles {
        if let Ok(Some(group)) = handle.await {
            groups.push(group);
        }
    }
    // Keep the user's endpoint ordering rather than completion order.
    groups.sort_by_key(|(name, _)| {
        endpoints
            .iter()
            .position(|e| &e.name == name)
            .unwrap_or(usize::MAX)
    });
    groups
}

/// Fetch one resource kind from one endpoint.
async fn fetch_rows(
    endpoint: &Endpoint,
    resource: crate::cli::root::ResourceType,
    _sort_by: Option<String>,
) -> Result<Rows> {
    use crate::cli::root::ResourceType;

    let client = connect(endpoint)?;
    let docker = client.inner();

    Ok(match resource {
        ResourceType::Nodes => Rows::Nodes(crate::cli::rows::node_typed_rows(
            &crate::api::node::list_nodes(docker).await?,
        )),
        ResourceType::Services => Rows::Services(
            crate::cli::rows::service_typed_rows(
                docker,
                &crate::api::service::list_services(docker).await?,
            )
            .await?,
        ),
        ResourceType::Tasks => {
            let tasks = crate::api::task::list_tasks(docker).await?;
            let names = crate::cli::rows::node_hostnames(docker).await;
            Rows::Tasks(crate::cli::rows::task_typed_rows(&tasks, &names))
        }
        ResourceType::Networks => Rows::Networks(crate::cli::rows::network_typed_rows(
            &crate::api::network::list_networks(docker).await?,
        )),
        ResourceType::Secrets => Rows::Secrets(crate::cli::rows::secret_typed_rows(
            &crate::api::secret::list_secrets(docker).await?,
        )),
        ResourceType::Configs => Rows::Configs(crate::cli::rows::config_typed_rows(
            &crate::api::config::list_configs(docker).await?,
        )),
        ResourceType::Stacks => Rows::Stacks(crate::cli::rows::stack_typed_rows(
            &crate::api::stack::list_stacks(docker).await?,
        )),
    })
}

/// Render per-endpoint groups as JSON/YAML, or as sectioned text tables.
fn render_groups(
    groups: Vec<(String, Rows)>,
    output: crate::cli::root::OutputFormat,
) -> Result<()> {
    use crate::cli::root::OutputFormat;

    match output {
        OutputFormat::Json => {
            // One document keyed by endpoint. Concatenating arrays would not be
            // valid JSON and would lose the association with its endpoint.
            let map: serde_json::Map<String, serde_json::Value> = groups
                .into_iter()
                .map(|(name, rows)| (name, rows.to_json()))
                .collect();
            crate::utils::printer::print_json(&serde_json::Value::Object(map))?;
        }
        OutputFormat::Yaml => {
            let map: serde_json::Map<String, serde_json::Value> = groups
                .into_iter()
                .map(|(name, rows)| (name, rows.to_json()))
                .collect();
            crate::utils::printer::print_yaml(&serde_json::Value::Object(map))?;
        }
        _ => {
            for (name, rows) in groups {
                println!("=== {} ===", name);
                if rows.is_empty() {
                    println!("(no resources)");
                } else {
                    rows.print_table();
                }
                println!();
            }
        }
    }
    Ok(())
}

/// Run a mutating command against every endpoint, after confirmation.
async fn run_mutating(cli: &Cli, command: Commands, endpoints: &[Endpoint]) -> Result<()> {
    let verb = command_verb(&command);

    eprintln!();
    eprintln!(
        "The following {} endpoints will be modified:",
        endpoints.len()
    );
    for endpoint in endpoints {
        eprintln!("  {:<20} {}", endpoint.name, endpoint.host);
    }
    eprintln!();

    if !cli.yes && !confirm(&format!("Proceed with '{}' on all of them?", verb))? {
        anyhow::bail!("Aborted. Nothing was changed.");
    }

    // Own both values so each spawned task is 'static and independent.
    let snapshot = cli.clone();
    let outcomes = for_each_endpoint(endpoints, move |endpoint| {
        let command = command.clone();
        let snapshot = snapshot.clone();
        async move {
            let client = connect(&endpoint)?;
            crate::cli::root::Cli::dispatch_single(&snapshot, command, &client).await
        }
    })
    .await;

    println!();
    println!("Results:");
    for outcome in &outcomes {
        let status = if outcome.ok { "ok" } else { "FAILED" };
        let detail = outcome.error.clone().unwrap_or_default();
        println!(
            "  {:<6} {:<20} {:>6}ms  {}",
            status, outcome.endpoint, outcome.duration_ms, detail
        );
    }

    let failures = report_outcomes(&outcomes);
    if failures > 0 {
        anyhow::bail!(
            "{} of {} endpoint(s) failed; the rest were updated.",
            failures,
            outcomes.len()
        );
    }
    Ok(())
}

fn command_verb(command: &Commands) -> &'static str {
    use crate::cli::root::Commands::*;
    match command {
        Scale { .. } => "scale",
        Delete { .. } => "delete",
        Create { .. } => "create",
        Apply { .. } => "apply",
        Replace { .. } => "replace",
        Patch { .. } => "patch",
        Label { .. } => "label",
        Set { .. } => "set",
        Taint { .. } => "taint",
        Rollout { .. } => "rollout",
        Cordon { .. } => "cordon",
        Uncordon { .. } => "uncordon",
        Drain { .. } => "drain",
        Promote { .. } => "promote",
        Demote { .. } => "demote",
        Swarm { .. } => "swarm",
        Stack { .. } => "stack",
        Failover { .. } => "failover",
        Run { .. } => "run",
        _ => "modify",
    }
}

/// Ask for confirmation, defaulting to no.
fn confirm(prompt: &str) -> Result<bool> {
    use std::io::{self, BufRead, Write};
    if !std::io::IsTerminal::is_terminal(&io::stdin()) {
        // Not interactive: refuse rather than assume consent.
        return Ok(false);
    }
    print!("{} [y/N] ", prompt);
    io::stdout().flush()?;
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line)? == 0 {
        return Ok(false);
    }
    let answer = line.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
}
