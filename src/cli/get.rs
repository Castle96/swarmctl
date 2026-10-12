use crate::api::client::DockerClient;
use crate::cli::kobject;
use crate::cli::registry::{self, ResourceType};
use crate::cli::root::OutputFormat;
use crate::models::{node::NodeRow, service::ServiceRow};
use crate::utils::printer::{RenderOpts, print_json, print_yaml, render_list, sort_order};
use crate::utils::selectors::{Selector, format_labels, keep_matching, validate_selector};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// Flags that shape how a resource list is queried and rendered.
///
/// Bundled into a single struct because every list handler takes the same set.
#[derive(Clone, Default)]
pub struct ListOptions {
    pub show_labels: bool,
    /// `--show-kind`: prefix `-o name` output with the resource type.
    pub show_kind: bool,
    /// `--no-headers`: omit the table header row.
    pub no_headers: bool,
    /// `-L/--label-columns`: extra label columns to display.
    pub label_columns: Vec<String>,
    pub selector: Option<String>,
    pub field_selector: Option<String>,
    pub sort_by: Option<String>,
    pub watch: bool,
}

impl ListOptions {
    fn render_opts(&self) -> RenderOpts {
        RenderOpts {
            show_labels: self.show_labels,
            show_kind: self.show_kind,
            no_headers: self.no_headers,
            label_columns: self.label_columns.clone(),
        }
    }

    /// Parse both selectors up front so a typo is reported before any API call.
    ///
    /// This is the fix for a genuinely nasty failure mode: `get svc --selector
    /// app==` used to match zero rows and look exactly like an empty swarm.
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_selector(self.selector.as_ref(), self.field_selector.as_ref())
    }

    /// Reject selectors that cannot apply to `resource`.
    ///
    /// Stacks have no API object and no labels, so `--selector` and
    /// `--field-selector` have nothing to match against. Ignoring them silently
    /// meant `get stacks --selector app=web` returned every stack; say so
    /// instead.
    pub fn reject_unsupported(&self, resource: ResourceType) -> anyhow::Result<()> {
        if resource != ResourceType::Stacks {
            return Ok(());
        }
        if self.selector.is_some() {
            anyhow::bail!(
                "--selector is not supported for stacks: they carry no labels. \
                 Address one by name instead: `get stacks <name>`"
            );
        }
        if self.field_selector.is_some() {
            anyhow::bail!(
                "--field-selector is not supported for stacks: a stack has no \
                 API fields to select on"
            );
        }
        Ok(())
    }
}

/// Run `get` for every requested resource.
///
/// `requests` comes from the registry and may hold several types, each with its
/// own names: `get po,svc`, `get po/web`, `get po web api` all arrive here.
pub async fn run(
    client: &DockerClient,
    requests: &[registry::ResourceRequest],
    output: OutputFormat,
    opts: ListOptions,
) -> anyhow::Result<()> {
    opts.validate()?;
    for req in requests {
        if req.names.is_empty() {
            opts.reject_unsupported(req.resource)?;
        }
    }

    let watch = opts.watch;
    loop {
        for req in requests {
            dispatch(client, req, &output, &opts).await?;
        }

        if !watch {
            break;
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        // Clear after the pause, immediately before redrawing. Clearing here
        // meant the table was visible for microseconds and the screen was then
        // blank for two seconds, which reads as a flicker.
        print!("\x1B[2J\x1B[1;1H");
    }

    Ok(())
}

async fn dispatch(
    client: &DockerClient,
    req: &registry::ResourceRequest,
    output: &OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    match req.resource {
        ResourceType::Nodes => {
            if req.names.is_empty() {
                get_nodes(client, output, opts).await
            } else {
                get_node(client, &req.names, output).await
            }
        }
        ResourceType::Services => {
            if req.names.is_empty() {
                get_services(client, output, opts).await
            } else {
                get_service(client, &req.names, output).await
            }
        }
        ResourceType::Tasks => {
            if req.names.is_empty() {
                get_tasks(client, output, opts).await
            } else {
                get_task(client, &req.names, output).await
            }
        }
        ResourceType::Networks => {
            if req.names.is_empty() {
                get_networks(client, output, opts).await
            } else {
                get_network(client, &req.names, output).await
            }
        }
        ResourceType::Secrets => {
            if req.names.is_empty() {
                get_secrets(client, output, opts).await
            } else {
                get_secret(client, &req.names, output).await
            }
        }
        ResourceType::Configs => {
            if req.names.is_empty() {
                get_configs(client, output, opts).await
            } else {
                get_config(client, &req.names, output).await
            }
        }
        ResourceType::Stacks => {
            if req.names.is_empty() {
                get_stacks(client, output, opts).await
            } else {
                get_stack(client, &req.names, output).await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// List pipeline
// ---------------------------------------------------------------------------

/// A filtered, ordered list of API objects, kept in both representations.
///
/// The two sides are returned together on purpose. An earlier version filtered
/// the serialized form and built display rows from the *original* list, so
/// `--field-selector` and named `get` silently did nothing for `-o table` and
/// `-o name` while quietly working for `-o json`. Handing both back as one value
/// makes that desynchronization unrepresentable.
struct Filtered<T> {
    /// Survivors, in render order — the input to the row builders.
    typed: Vec<T>,
    /// The same survivors serialized, in the same order — the input to the
    /// Kubernetes envelope.
    raw: Vec<Value>,
}

/// Filter by name and `--field-selector`, order by `--sort-by`, then serialize.
fn filter_and_sort<T: Serialize + Clone>(
    resource: ResourceType,
    typed: Vec<T>,
    names: &[String],
    opts: &ListOptions,
) -> anyhow::Result<Filtered<T>> {
    let field_sel = opts
        .field_selector
        .as_deref()
        .map(Selector::parse)
        .transpose()
        .map_err(|e| anyhow::anyhow!("--field-selector: {e}"))?;

    let mut kept: Vec<T> = Vec::with_capacity(typed.len());
    let mut raw: Vec<Value> = Vec::with_capacity(typed.len());

    for item in typed {
        let value = to_value(&item)?;
        if !names.is_empty() && !names.iter().any(|n| name_matches(resource, &value, n)) {
            continue;
        }
        if let Some(sel) = &field_sel {
            let envelope = kobject::to_k8s(resource, &value);
            if !sel.matches_value(&envelope) {
                continue;
            }
        }
        raw.push(value);
        kept.push(item);
    }

    let Some(sort_field) = opts.sort_by.as_deref() else {
        return Ok(Filtered { typed: kept, raw });
    };

    // Sort one side and apply the identical permutation to the other.
    let order = sort_order(resource, &raw, sort_field)?;
    Ok(Filtered {
        typed: order.iter().map(|i| kept[*i].clone()).collect(),
        raw: order.iter().map(|i| raw[*i].clone()).collect(),
    })
}

/// Does this object answer to `name`?
///
/// Tasks are matched on their own `Name` and their id as well as the envelope
/// name, because `get po <task-id>` is a thing people do.
fn name_matches(resource: ResourceType, item: &Value, name: &str) -> bool {
    let envelope = kobject::to_k8s(resource, item);
    if envelope["metadata"]["name"].as_str() == Some(name) {
        return true;
    }
    if item.get("ID").and_then(Value::as_str) == Some(name) {
        return true;
    }
    // Stacks are synthesized, not fetched, so their name lives elsewhere.
    if resource == ResourceType::Stacks
        && item
            .get("metadata")
            .and_then(|m| m.get("name"))
            .and_then(Value::as_str)
            == Some(name)
    {
        return true;
    }
    if resource == ResourceType::Tasks && item.get("Name").and_then(Value::as_str) == Some(name) {
        return true;
    }
    false
}

/// The names a named `get` should error on when nothing matched.
fn not_found(resource: ResourceType, names: &[String]) -> anyhow::Error {
    let kind = registry::singular(resource);
    anyhow::anyhow!(
        "{} {} not found",
        kind,
        names
            .iter()
            .map(|n| format!("'{n}'"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Report an empty result the way kubectl does.
fn report_empty(resource: ResourceType, names: &[String]) {
    if names.is_empty() {
        println!("No resources found.");
    } else {
        println!(
            "No resources found for {} {}.",
            registry::singular(resource),
            names.join(", ")
        );
    }
}

// ---------------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------------

async fn get_nodes(
    client: &DockerClient,
    output: &OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;
    let nodes = keep_matching(
        nodes,
        |n: &bollard::models::Node| n.spec.as_ref().and_then(|s| s.labels.clone()),
        opts.selector.as_ref(),
        None,
    );
    let kept = filter_and_sort(ResourceType::Nodes, nodes, &[], opts)?;
    let rows: Vec<NodeRow> = crate::cli::rows::node_rows(&kept.typed);
    let items = kept.raw;

    if rows.is_empty() {
        report_empty(ResourceType::Nodes, &[]);
        return Ok(());
    }
    render_list(
        ResourceType::Nodes,
        &items,
        &rows,
        output,
        &opts.render_opts(),
    )
}

async fn get_node(
    client: &DockerClient,
    names: &[String],
    output: &OutputFormat,
) -> anyhow::Result<()> {
    let nodes = crate::api::node::list_nodes(client.inner()).await?;
    let found: Vec<bollard::models::Node> = nodes
        .iter()
        .filter(|n| {
            let raw = serde_json::to_value(n).unwrap_or(Value::Null);
            names
                .iter()
                .any(|want| name_matches(ResourceType::Nodes, &raw, want))
        })
        .cloned()
        .collect();
    let first = found
        .first()
        .ok_or_else(|| not_found(ResourceType::Nodes, names))?;
    let raw = to_value(first)?;
    let items = vec![raw.clone()];

    match output {
        OutputFormat::Table | OutputFormat::Wide => {
            print_node_detail(first);
        }
        _ => render_single(ResourceType::Nodes, &items, output),
    }
    Ok(())
}

fn print_node_detail(node: &bollard::models::Node) {
    let spec = node.spec.as_ref().cloned().unwrap_or_default();
    let status = node.status.clone().unwrap_or_default();
    let hostname = node
        .description
        .as_ref()
        .and_then(|d| d.hostname.clone())
        .or_else(|| spec.name.clone())
        .unwrap_or_default();
    println!("Name: {}", hostname);
    println!("ID: {}", node.id.clone().unwrap_or_default());
    println!(
        "Status: {}",
        status.state.unwrap_or(bollard::models::NodeState::READY)
    );
    println!(
        "Availability: {}",
        spec.availability
            .unwrap_or(bollard::models::NodeSpecAvailabilityEnum::ACTIVE)
    );
    if let Some(labels) = &spec.labels
        && !labels.is_empty()
    {
        println!("Labels: {}", format_labels(&Some(labels.clone())));
    }
    if let Some(manager_status) = &node.manager_status {
        println!(
            "Manager Status: {}",
            manager_status
                .reachability
                .unwrap_or(bollard::models::Reachability::UNKNOWN)
        );
    }
}

// ---------------------------------------------------------------------------
// Services
// ---------------------------------------------------------------------------

async fn get_services(
    client: &DockerClient,
    output: &OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let services = keep_matching(
        services,
        |s: &bollard::models::Service| s.spec.as_ref().and_then(|sp| sp.labels.clone()),
        opts.selector.as_ref(),
        None,
    );
    let kept = filter_and_sort(ResourceType::Services, services, &[], opts)?;
    let rows: Vec<ServiceRow> = crate::cli::rows::service_rows(client.inner(), &kept.typed).await?;
    let items = kept.raw;

    if rows.is_empty() {
        report_empty(ResourceType::Services, &[]);
        return Ok(());
    }
    render_list(
        ResourceType::Services,
        &items,
        &rows,
        output,
        &opts.render_opts(),
    )
}

async fn get_service(
    client: &DockerClient,
    names: &[String],
    output: &OutputFormat,
) -> anyhow::Result<()> {
    let services = crate::api::service::list_services(client.inner()).await?;
    let mut items = Vec::new();
    let mut detail = None;
    for s in &services {
        let raw = to_value(s)?;
        if names
            .iter()
            .any(|n| name_matches(ResourceType::Services, &raw, n))
        {
            if detail.is_none() {
                detail = Some(s.clone());
            }
            items.push(raw);
        }
    }
    let first = detail.ok_or_else(|| not_found(ResourceType::Services, names))?;

    match output {
        OutputFormat::Table | OutputFormat::Wide => print_service_detail(&first),
        _ => render_single(ResourceType::Services, &items, output),
    }
    Ok(())
}

fn print_service_detail(service: &bollard::models::Service) {
    let spec = service.spec.clone().unwrap_or_default();
    println!("Name: {}", spec.name.clone().unwrap_or_default());
    println!("ID: {}", service.id.clone().unwrap_or_default());
    if let Some(labels) = &spec.labels
        && !labels.is_empty()
    {
        println!("Labels: {}", format_labels(&Some(labels.clone())));
    }
    if let Some(mode) = &spec.mode
        && let Some(replicated) = &mode.replicated
    {
        println!("Replicas: {}", replicated.replicas.unwrap_or(0));
    }
    if let Some(task_template) = &spec.task_template
        && let Some(container_spec) = &task_template.container_spec
    {
        println!(
            "Image: {}",
            container_spec.image.clone().unwrap_or_default()
        );
    }
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

async fn get_tasks(
    client: &DockerClient,
    output: &OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;

    // Resolve node IDs to hostnames so the NODE column lines up with `get nodes`.
    let node_hostnames: HashMap<String, String> =
        match crate::api::node::list_nodes(client.inner()).await {
            Ok(nodes) => nodes
                .into_iter()
                .filter_map(|n| {
                    let id = n.id?;
                    let hostname = n
                        .description
                        .as_ref()
                        .and_then(|d| d.hostname.clone())
                        .or_else(|| n.spec.as_ref().and_then(|s| s.name.clone()))
                        .unwrap_or_else(|| id.clone());
                    Some((id, hostname))
                })
                .collect(),
            // Non-fatal: fall back to showing raw node IDs.
            Err(e) => {
                log::warn!("Could not resolve node hostnames: {}", e);
                Default::default()
            }
        };

    let tasks = keep_matching(
        tasks,
        |t: &bollard::models::Task| {
            t.spec
                .as_ref()
                .and_then(|s| s.container_spec.as_ref())
                .and_then(|c| c.labels.clone())
        },
        opts.selector.as_ref(),
        None,
    );
    let kept = filter_and_sort(ResourceType::Tasks, tasks, &[], opts)?;
    let rows = crate::cli::rows::task_rows_with_nodes(&kept.typed, &node_hostnames);
    let items = kept.raw;

    if rows.is_empty() {
        report_empty(ResourceType::Tasks, &[]);
        return Ok(());
    }
    render_list(
        ResourceType::Tasks,
        &items,
        &rows,
        output,
        &opts.render_opts(),
    )
}

async fn get_task(
    client: &DockerClient,
    names: &[String],
    output: &OutputFormat,
) -> anyhow::Result<()> {
    let tasks = crate::api::task::list_tasks(client.inner()).await?;
    let mut items = Vec::new();
    let mut detail = None;
    for t in &tasks {
        let raw = to_value(t)?;
        if names
            .iter()
            .any(|n| name_matches(ResourceType::Tasks, &raw, n))
        {
            if detail.is_none() {
                detail = Some(t.clone());
            }
            items.push(raw);
        }
    }
    let first = detail.ok_or_else(|| not_found(ResourceType::Tasks, names))?;

    match output {
        OutputFormat::Table | OutputFormat::Wide => print_task_detail(&first),
        _ => render_single(ResourceType::Tasks, &items, output),
    }
    Ok(())
}

fn print_task_detail(task: &bollard::models::Task) {
    println!("ID: {}", task.id.clone().unwrap_or_default());
    println!("Name: {}", task.name.clone().unwrap_or_default());
    let labels = task
        .spec
        .as_ref()
        .and_then(|s| s.container_spec.as_ref())
        .and_then(|c| c.labels.clone());
    if let Some(lbls) = &labels
        && !lbls.is_empty()
    {
        println!("Labels: {}", format_labels(&Some(lbls.clone())));
    }
    if let Some(status) = &task.status {
        println!("Status: {}", status.state.unwrap_or_default());
        if let Some(message) = &status.message {
            println!("Message: {}", message);
        }
    }
    println!("Desired State: {}", task.desired_state.unwrap_or_default());
    println!("Node ID: {}", task.node_id.clone().unwrap_or_default());
    if let Some(spec) = &task.spec
        && let Some(container_spec) = &spec.container_spec
    {
        println!(
            "Image: {}",
            container_spec
                .image
                .clone()
                .unwrap_or_else(|| "unknown".into())
        );
    }
}

// ---------------------------------------------------------------------------
// Networks
// ---------------------------------------------------------------------------

async fn get_networks(
    client: &DockerClient,
    output: &OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    let networks = crate::api::network::list_networks(client.inner()).await?;
    let networks = keep_matching(
        networks,
        |n: &bollard::models::Network| n.labels.clone(),
        opts.selector.as_ref(),
        None,
    );
    let kept = filter_and_sort(ResourceType::Networks, networks, &[], opts)?;
    let rows = crate::cli::rows::network_rows(&kept.typed);
    let items = kept.raw;

    if rows.is_empty() {
        report_empty(ResourceType::Networks, &[]);
        return Ok(());
    }
    render_list(
        ResourceType::Networks,
        &items,
        &rows,
        output,
        &opts.render_opts(),
    )
}

async fn get_network(
    client: &DockerClient,
    names: &[String],
    output: &OutputFormat,
) -> anyhow::Result<()> {
    let networks = crate::api::network::list_networks(client.inner()).await?;
    let mut items = Vec::new();
    let mut detail = None;
    for n in &networks {
        let raw = to_value(n)?;
        if names
            .iter()
            .any(|w| name_matches(ResourceType::Networks, &raw, w))
        {
            if detail.is_none() {
                detail = Some(n.clone());
            }
            items.push(raw);
        }
    }
    let first = detail.ok_or_else(|| not_found(ResourceType::Networks, names))?;

    match output {
        OutputFormat::Table | OutputFormat::Wide => {
            println!(
                "Name: {}",
                first.name.clone().unwrap_or_else(|| "unknown".into())
            );
            println!("ID: {}", first.id.clone().unwrap_or_default());
            println!(
                "Driver: {}",
                first.driver.clone().unwrap_or_else(|| "unknown".into())
            );
            println!(
                "Scope: {}",
                first.scope.clone().unwrap_or_else(|| "unknown".into())
            );
            println!("Internal: {}", first.internal.unwrap_or(false));
            if let Some(labels) = &first.labels
                && !labels.is_empty()
            {
                println!("Labels: {}", format_labels(&Some(labels.clone())));
            }
        }
        _ => render_single(ResourceType::Networks, &items, output),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

async fn get_secrets(
    client: &DockerClient,
    output: &OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    let secrets = crate::api::secret::list_secrets(client.inner()).await?;
    let secrets = keep_matching(
        secrets,
        |s: &bollard::models::Secret| s.spec.as_ref().and_then(|sp| sp.labels.clone()),
        opts.selector.as_ref(),
        None,
    );
    let kept = filter_and_sort(ResourceType::Secrets, secrets, &[], opts)?;
    let rows = crate::cli::rows::secret_rows(&kept.typed);
    let items = kept.raw;

    if rows.is_empty() {
        report_empty(ResourceType::Secrets, &[]);
        return Ok(());
    }
    render_list(
        ResourceType::Secrets,
        &items,
        &rows,
        output,
        &opts.render_opts(),
    )
}

async fn get_secret(
    client: &DockerClient,
    names: &[String],
    output: &OutputFormat,
) -> anyhow::Result<()> {
    let secrets = crate::api::secret::list_secrets(client.inner()).await?;
    let mut items = Vec::new();
    let mut detail = None;
    for s in &secrets {
        let raw = to_value(s)?;
        if names
            .iter()
            .any(|n| name_matches(ResourceType::Secrets, &raw, n))
        {
            if detail.is_none() {
                detail = Some(s.clone());
            }
            items.push(raw);
        }
    }
    let first = detail.ok_or_else(|| not_found(ResourceType::Secrets, names))?;

    match output {
        OutputFormat::Table | OutputFormat::Wide => print_named_detail(
            "Secret",
            first.spec.as_ref().and_then(|s| s.name.clone()),
            first.id.clone(),
            first.created_at.clone(),
            first.spec.as_ref().and_then(|s| s.labels.clone()),
        ),
        _ => render_single(ResourceType::Secrets, &items, output),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Configs
// ---------------------------------------------------------------------------

async fn get_configs(
    client: &DockerClient,
    output: &OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    let configs = crate::api::config::list_configs(client.inner()).await?;
    let configs = keep_matching(
        configs,
        |c: &bollard::models::Config| c.spec.as_ref().and_then(|sp| sp.labels.clone()),
        opts.selector.as_ref(),
        None,
    );
    let kept = filter_and_sort(ResourceType::Configs, configs, &[], opts)?;
    let rows = crate::cli::rows::config_rows(&kept.typed);
    let items = kept.raw;

    if rows.is_empty() {
        report_empty(ResourceType::Configs, &[]);
        return Ok(());
    }
    render_list(
        ResourceType::Configs,
        &items,
        &rows,
        output,
        &opts.render_opts(),
    )
}

async fn get_config(
    client: &DockerClient,
    names: &[String],
    output: &OutputFormat,
) -> anyhow::Result<()> {
    let configs = crate::api::config::list_configs(client.inner()).await?;
    let mut items = Vec::new();
    let mut detail = None;
    for c in &configs {
        let raw = to_value(c)?;
        if names
            .iter()
            .any(|n| name_matches(ResourceType::Configs, &raw, n))
        {
            if detail.is_none() {
                detail = Some(c.clone());
            }
            items.push(raw);
        }
    }
    let first = detail.ok_or_else(|| not_found(ResourceType::Configs, names))?;

    match output {
        OutputFormat::Table | OutputFormat::Wide => print_named_detail(
            "Config",
            first.spec.as_ref().and_then(|s| s.name.clone()),
            first.id.clone(),
            first.created_at.clone(),
            first.spec.as_ref().and_then(|s| s.labels.clone()),
        ),
        _ => render_single(ResourceType::Configs, &items, output),
    }
    Ok(())
}

fn print_named_detail(
    kind: &str,
    name: Option<String>,
    id: Option<String>,
    created_at: Option<String>,
    labels: Option<HashMap<String, String>>,
) {
    println!("{}: {}", kind, name.unwrap_or_else(|| "unknown".into()));
    println!("ID: {}", id.unwrap_or_default());
    println!("Created At: {}", created_at.unwrap_or_default());
    if let Some(l) = labels
        && !l.is_empty()
    {
        println!("Labels: {}", format_labels(&Some(l)));
    }
}

// ---------------------------------------------------------------------------
// Stacks
// ---------------------------------------------------------------------------

async fn get_stack(
    client: &DockerClient,
    names: &[String],
    output: &OutputFormat,
) -> anyhow::Result<()> {
    let stacks = crate::api::stack::list_stacks(client.inner()).await?;
    let found: Vec<_> = stacks
        .iter()
        .filter(|s| names.contains(&s.name))
        .cloned()
        .collect();
    if found.is_empty() {
        return Err(not_found(ResourceType::Stacks, names));
    }
    let items = stacks_to_values(&found);
    let rows = crate::cli::rows::stack_rows(&found);
    render_list(
        ResourceType::Stacks,
        &items,
        &rows,
        output,
        &RenderOpts::default(),
    )
}

async fn get_stacks(
    client: &DockerClient,
    output: &OutputFormat,
    opts: &ListOptions,
) -> anyhow::Result<()> {
    // Stacks carry no labels and no API object to select against, so
    // `--selector`/`--field-selector` cannot apply to them. Only NAME and
    // `--sort-by` are meaningful, and both are honoured.
    let mut stacks = crate::api::stack::list_stacks(client.inner()).await?;
    let mut items = stacks_to_values(&stacks);
    if let Some(sort_field) = opts.sort_by.as_deref() {
        let order = sort_order(ResourceType::Stacks, &items, sort_field)?;
        items = order.iter().map(|i| items[*i].clone()).collect();
        stacks = order.iter().map(|i| stacks[*i].clone()).collect();
    }
    let rows = crate::cli::rows::stack_rows(&stacks);

    if rows.is_empty() {
        report_empty(ResourceType::Stacks, &[]);
        return Ok(());
    }
    render_list(
        ResourceType::Stacks,
        &items,
        &rows,
        output,
        &opts.render_opts(),
    )
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Render exactly one object in a machine format.
///
/// Named `get` keeps its existing human-readable detail view for `table`/`wide`;
/// every other format goes through the Kubernetes envelope so `-o json`,
/// `-o jsonpath` and `--field-selector` all agree.
fn render_single(resource: ResourceType, items: &[Value], output: &OutputFormat) {
    let envelope = kobject::to_k8s_list(resource, items);
    let _ = match output {
        OutputFormat::Json => print_json(&envelope),
        OutputFormat::Yaml => print_yaml(&envelope),
        OutputFormat::JsonPath(t) => {
            crate::utils::jsonpath::execute(t, &envelope).map(|s| println!("{s}"))
        }
        OutputFormat::JsonPathAsJson(t) => crate::utils::jsonpath::parse(t)
            .and_then(|p| p.eval(&envelope))
            .and_then(|v| print_json(&v)),
        OutputFormat::GoTemplate(t) => {
            crate::utils::jsonpath::execute(t, &envelope).map(|s| println!("{s}"))
        }
        OutputFormat::CustomColumns(t) => render_single_columns(&envelope, t),
        // Table/Wide/Name are handled by the caller.
        OutputFormat::Table | OutputFormat::Wide | OutputFormat::Name => Ok(()),
    };
}

fn render_single_columns(envelope: &Value, spec: &str) -> anyhow::Result<()> {
    let (_, paths) = crate::utils::printer::parse_custom_columns(spec)?;
    let items = envelope
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for item in items {
        let cells: Vec<String> = paths
            .iter()
            .map(|p| crate::utils::jsonpath::eval_text(p, &item).unwrap_or_default())
            .collect();
        println!("{}", cells.join("\t"));
    }
    Ok(())
}

fn to_value<T: Serialize>(item: &T) -> anyhow::Result<Value> {
    serde_json::to_value(item).map_err(Into::into)
}

/// Stacks are synthesized from services, not fetched from the daemon, so they
/// need their own adapter into the API-object shape.
///
/// It emits **raw Docker-style keys**, not a finished envelope: every other
/// resource goes through [`kobject::to_k8s`], and `render_list` calls that
/// uniformly. Emitting an envelope here produced a double-wrapped object whose
/// `spec` was `null` and whose `metadata.name` was empty.
fn stacks_to_values(stacks: &[crate::api::stack::StackSummary]) -> Vec<Value> {
    stacks
        .iter()
        .map(|s| {
            serde_json::json!({
                "ID": s.name,
                "Spec": {
                    "Name": s.name,
                    "Services": s.services,
                    "Replicas": s.replicas,
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::registry::ResourceRequest;
    use serde_json::json;

    #[test]
    fn list_options_reject_selectors_for_stacks() {
        let opts = ListOptions {
            selector: Some("app=web".into()),
            ..Default::default()
        };
        let err = opts.reject_unsupported(ResourceType::Stacks).unwrap_err();
        assert!(err.to_string().contains("--selector"), "{err}");
        // Other resources are unaffected.
        assert!(opts.reject_unsupported(ResourceType::Services).is_ok());
    }

    #[test]
    fn list_options_reject_field_selectors_for_stacks() {
        let opts = ListOptions {
            field_selector: Some("a=b".into()),
            ..Default::default()
        };
        assert!(opts.reject_unsupported(ResourceType::Stacks).is_err());
    }

    #[test]
    fn malformed_selectors_fail_validation_before_any_api_call() {
        let opts = ListOptions {
            selector: Some("app in nope".into()),
            ..Default::default()
        };
        let err = opts.validate().unwrap_err().to_string();
        assert!(err.contains("--selector"), "{err}");
    }

    #[test]
    fn render_opts_reflect_the_list_flags() {
        let opts = ListOptions {
            show_labels: true,
            show_kind: true,
            no_headers: true,
            label_columns: vec!["app".into()],
            ..Default::default()
        };
        let r = opts.render_opts();
        assert!(r.show_labels && r.show_kind && r.no_headers);
        assert_eq!(r.label_columns, vec!["app"]);
    }

    #[test]
    fn not_found_names_the_kind_and_the_missing_names() {
        let err = not_found(
            ResourceType::Services,
            &["web".to_string(), "api".to_string()],
        )
        .to_string();
        assert!(err.contains("service"), "{err}");
        assert!(err.contains("'web'"), "{err}");
        assert!(err.contains("'api'"), "{err}");
    }

    fn svc(id: &str, name: &str, replicas: u64) -> Value {
        serde_json::json!({
            "ID": id,
            "Spec": {"Name": name, "Mode": {"Replicated": {"Replicas": replicas}}}
        })
    }

    /// `rows` and the rendered envelopes must be the same set, or `-o table`
    /// silently ignores what `-o json` honours.
    #[test]
    fn typed_and_serialized_sides_stay_aligned() {
        let typed = vec![svc("x", "web", 3), svc("y", "api", 10)];
        let opts = ListOptions {
            field_selector: Some("spec.replicas=10".into()),
            ..Default::default()
        };
        let kept = filter_and_sort(ResourceType::Services, typed, &[], &opts).unwrap();
        assert_eq!(kept.typed.len(), 1);
        assert_eq!(kept.raw.len(), 1);
        assert_eq!(kept.raw[0]["ID"], serde_json::json!("y"));
        assert_eq!(
            kobject::name_of(ResourceType::Services, &kept.raw[0]).unwrap(),
            "api"
        );
    }

    #[test]
    fn name_filtering_applies_to_both_sides() {
        let typed = vec![svc("x", "web", 1), svc("y", "api", 1)];
        let kept = filter_and_sort(
            ResourceType::Services,
            typed,
            &["api".into()],
            &ListOptions::default(),
        )
        .unwrap();
        assert_eq!(kept.typed.len(), 1);
        assert_eq!(kept.raw.len(), 1);
        assert_eq!(kept.raw[0]["Spec"]["Name"], serde_json::json!("api"));
    }

    #[test]
    fn sorting_applies_the_same_permutation_to_both_sides() {
        let typed = vec![svc("x", "web", 10), svc("y", "api", 2)];
        let opts = ListOptions {
            sort_by: Some("{.spec.replicas}".into()),
            ..Default::default()
        };
        let kept = filter_and_sort(ResourceType::Services, typed, &[], &opts).unwrap();
        // 2 must come before 10, and both sides must agree on which is first.
        assert_eq!(kept.raw[0]["Spec"]["Name"], serde_json::json!("api"));
        assert_eq!(kept.typed[0]["Spec"]["Name"], serde_json::json!("api"));
    }

    #[test]
    fn a_field_selector_that_matches_nothing_yields_no_rows() {
        let typed = vec![svc("x", "web", 3)];
        let opts = ListOptions {
            field_selector: Some("spec.replicas=999".into()),
            ..Default::default()
        };
        let kept = filter_and_sort(ResourceType::Services, typed, &[], &opts).unwrap();
        assert!(kept.typed.is_empty());
        assert!(kept.raw.is_empty());
    }

    /// A bare key that does not exist must filter everything out. It used to
    /// return every row, which is why the README's own example was wrong.
    #[test]
    fn field_selector_against_the_wrong_path_matches_nothing() {
        let typed = vec![svc("x", "web", 3)];
        let opts = ListOptions {
            field_selector: Some("availability=active".into()),
            ..Default::default()
        };
        let kept = filter_and_sort(ResourceType::Services, typed, &[], &opts).unwrap();
        assert!(kept.typed.is_empty());
    }

    #[test]
    fn name_matches_covers_id_and_task_name() {
        let task = serde_json::json!({"ID": "tid", "Name": "web.1.abc"});
        assert!(name_matches(ResourceType::Tasks, &task, "tid"));
        assert!(name_matches(ResourceType::Tasks, &task, "web.1.abc"));
        assert!(!name_matches(ResourceType::Tasks, &task, "nope"));
    }

    /// Stacks are synthesized, so they bypass the API-object path entirely.
    /// They must still come out as a single, correctly-keyed envelope — an
    /// earlier adapter pre-built the envelope itself and `render_list` wrapped
    /// it a second time, yielding `spec: null` and an empty `metadata.name`.
    #[test]
    fn synthesized_stacks_get_one_envelope_not_two() {
        let stacks = vec![crate::api::stack::StackSummary {
            name: "mcp".into(),
            services: 4,
            replicas: 4,
        }];
        let raw = stacks_to_values(&stacks);
        let list = kobject::to_k8s_list(ResourceType::Stacks, &raw);

        assert_eq!(list["kind"], json!("StackList"));
        assert_eq!(list["items"][0]["kind"], json!("Stack"));
        assert_eq!(list["items"][0]["metadata"]["name"], json!("mcp"));
        assert_eq!(list["items"][0]["spec"]["services"], json!(4));
        assert_eq!(list["items"][0]["spec"]["replicas"], json!(4));
        // The double-wrap showed up as a null spec; assert it is an object.
        assert!(list["items"][0]["spec"].is_object());
    }

    #[test]
    fn resource_requests_drive_the_dispatch_shape() {
        let reqs = registry::parse_request("po,svc/web", &[], "v1.52").unwrap();
        assert_eq!(reqs.len(), 2);
        assert_eq!(
            reqs[0],
            ResourceRequest {
                resource: ResourceType::Tasks,
                names: vec![]
            }
        );
        assert_eq!(
            reqs[1],
            ResourceRequest {
                resource: ResourceType::Services,
                names: vec!["web".into()]
            }
        );
    }
}
