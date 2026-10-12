pub mod discover;

use crate::api::client::DockerClient;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::*,
};
use std::io;
use std::time::{Duration, Instant};

const COLOR_PRIMARY: Color = Color::Cyan;
const COLOR_SUCCESS: Color = Color::Green;
const COLOR_WARNING: Color = Color::Yellow;
const COLOR_ERROR: Color = Color::Red;
const COLOR_DIM: Color = Color::DarkGray;
const COLOR_HEADER_BG: Color = Color::Rgb(40, 44, 52);
const COLOR_SELECTION_BG: Color = Color::Rgb(50, 54, 62);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppState {
    Services,
    Nodes,
    Networks,
    Ports,
    Secrets,
    Tasks,
    Events,
    Logs,
}

impl AppState {
    fn all() -> &'static [AppState] {
        &[
            AppState::Services,
            AppState::Nodes,
            AppState::Networks,
            AppState::Ports,
            AppState::Secrets,
            AppState::Tasks,
            AppState::Events,
            AppState::Logs,
        ]
    }

    fn index(&self) -> usize {
        Self::all().iter().position(|s| s == self).unwrap_or(0)
    }

    fn label(&self) -> &'static str {
        match self {
            AppState::Services => "Services",
            AppState::Nodes => "Nodes",
            AppState::Networks => "Networks",
            AppState::Ports => "Ports",
            AppState::Secrets => "Secrets",
            AppState::Tasks => "Tasks",
            AppState::Events => "Events",
            AppState::Logs => "Logs",
        }
    }

    fn key(&self) -> &'static str {
        match self {
            AppState::Services => "1",
            AppState::Nodes => "2",
            AppState::Networks => "3",
            AppState::Ports => "4",
            AppState::Secrets => "5",
            AppState::Tasks => "6",
            AppState::Events => "7",
            AppState::Logs => "8",
        }
    }

    fn next(&self) -> AppState {
        let all = Self::all();
        let next_idx = (self.index() + 1) % all.len();
        all[next_idx]
    }

    fn prev(&self) -> AppState {
        let all = Self::all();
        let prev_idx = if self.index() == 0 {
            all.len() - 1
        } else {
            self.index() - 1
        };
        all[prev_idx]
    }
}

pub struct App {
    pub state: AppState,
    pub services: Vec<crate::models::service::ServiceRow>,
    pub nodes: Vec<crate::models::node::NodeRow>,
    pub networks: Vec<crate::models::network::NetworkRow>,
    pub ports: Vec<crate::models::port::ServicePortInfo>,
    pub secrets: Vec<crate::models::secret::SecretRow>,
    pub tasks: Vec<crate::models::task::TaskRow>,
    pub events: Vec<EventEntry>,
    pub selected_index: usize,
    pub detail_open: bool,
    pub search_mode: bool,
    pub search_query: String,
    pub auto_refresh: bool,
    pub auto_refresh_interval: u64,
    pub last_refresh: Instant,
    pub host_info: String,
    pub logs_services: Vec<(String, String)>,
    pub selected_log_service: usize,
    pub current_logs: String,
    pub total_items: usize,
}

#[derive(Debug, Clone)]
pub struct EventEntry {
    pub time: String,
    pub kind: String,
    pub action: String,
    pub id: String,
}

impl App {
    pub fn new(host_info: String) -> Self {
        Self {
            state: AppState::Services,
            services: Vec::new(),
            nodes: Vec::new(),
            networks: Vec::new(),
            ports: Vec::new(),
            secrets: Vec::new(),
            tasks: Vec::new(),
            events: Vec::new(),
            selected_index: 0,
            detail_open: false,
            search_mode: false,
            search_query: String::new(),
            auto_refresh: true,
            auto_refresh_interval: 5,
            last_refresh: Instant::now(),
            host_info,
            logs_services: Vec::new(),
            selected_log_service: 0,
            current_logs: String::new(),
            total_items: 0,
        }
    }

    pub fn status_icon(status: &str) -> (String, Color) {
        match status.to_lowercase().as_str() {
            "running" | "ready" | "active" | "up" => ("●".into(), COLOR_SUCCESS),
            "pending" | "waiting" | "starting" | "new" | "assigned" | "accepted" => {
                ("○".into(), COLOR_WARNING)
            }
            "failed" | "error" | "down" | "orphaned" | "remove" => ("●".into(), COLOR_ERROR),
            "paused" | "drain" | "draining" | "halted" => ("⏸".into(), COLOR_DIM),
            "complete" | "shutdown" => ("✓".into(), COLOR_DIM),
            _ => ("●".into(), COLOR_DIM),
        }
    }

    /// Rows matching the current search, in input order.
    ///
    /// The single search predicate. Previously this logic was written out
    /// fifteen times — once per `render_*` function plus `filtered_item_count` —
    /// and the copies had already drifted: `filtered_item_count` returned the
    /// *unfiltered* total for the Ports, Events and Logs tabs even though those
    /// renderers do filter, so the header count was wrong on three tabs.
    /// Status icon for the replicas column, which is `running/desired`.
    ///
    /// `status_icon` was being passed this string directly, but it matches on
    /// words like "running" and "ready" — `"1/3"` matched nothing, so every
    /// service dot was rendered dim, whether healthy or degraded.
    pub fn replica_icon(replicas: &str) -> (String, ratatui::style::Color) {
        let mut parts = replicas.splitn(2, '/');
        let running: Option<u64> = parts.next().and_then(|r| r.trim().parse().ok());
        let desired: Option<u64> = parts.next().and_then(|d| d.trim().parse().ok());

        match (running, desired) {
            // No replica count: a global service, which is neither healthy nor
            // degraded.
            (None, _) | (_, None) => ("●".into(), COLOR_DIM),
            (Some(_), Some(0)) => ("○".into(), COLOR_DIM),
            (Some(r), Some(d)) if r >= d => ("●".into(), COLOR_SUCCESS),
            // Fewer running than desired: degraded, which is the case the
            // dashboard previously hid.
            (Some(_), Some(_)) => ("●".into(), COLOR_ERROR),
        }
    }

    /// Rows matching the current search, in input order.
    fn filtered<'a, T>(&'a self, items: &'a [T], field: impl Fn(&T) -> &str) -> Vec<&'a T> {
        if self.search_query.is_empty() {
            return items.iter().collect();
        }
        let needle = self.search_query.to_lowercase();
        items
            .iter()
            .filter(|item| field(item).to_lowercase().contains(&needle))
            .collect()
    }

    /// Like [`Self::filtered`], matching any of several fields.
    fn filtered_multi<'a, T, const N: usize>(
        &'a self,
        items: &'a [T],
        fields: impl Fn(&'a T) -> [&'a str; N],
    ) -> Vec<&'a T> {
        if self.search_query.is_empty() {
            return items.iter().collect();
        }
        let needle = self.search_query.to_lowercase();
        items
            .iter()
            .filter(|item| {
                fields(item)
                    .iter()
                    .any(|f| f.to_lowercase().contains(&needle))
            })
            .collect()
    }

    /// How many rows the active tab is showing after the search filter.
    fn filtered_item_count(&self) -> usize {
        match self.state {
            AppState::Services => self.filtered(&self.services, |s| &s.name).len(),
            AppState::Nodes => self.filtered(&self.nodes, |n| &n.hostname).len(),
            AppState::Networks => self.filtered(&self.networks, |n| &n.name).len(),
            AppState::Secrets => self.filtered(&self.secrets, |s| &s.name).len(),
            AppState::Tasks => self.filtered(&self.tasks, |t| &t.name).len(),
            AppState::Ports => self.filtered(&self.ports, |p| &p.service_name).len(),
            AppState::Events => self
                .filtered_multi(&self.events, |e| [&e.action, &e.id])
                .len(),
            // The Logs pane selects a service rather than a row.
            AppState::Logs => self.logs_services.len(),
        }
    }

    /// Move the cursor down one row.
    ///
    /// On the Logs tab the left pane is a list of services and `selected_index`
    /// does nothing there: the pane highlights `selected_log_service`, which
    /// nothing else ever incremented. So the Logs tab could not be navigated at
    /// all. Both cursors now move together.
    fn move_selection_down(&mut self) {
        let total = self.filtered_item_count();
        if total > 0 {
            self.selected_index = (self.selected_index + 1).min(total - 1);
        }
        if !self.logs_services.is_empty() {
            self.selected_log_service =
                (self.selected_log_service + 1).min(self.logs_services.len() - 1);
        }
    }

    /// Move the cursor up one row.
    fn move_selection_up(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
        self.selected_log_service = self.selected_log_service.saturating_sub(1);
    }

    /// Keep the cursor inside the visible rows.
    ///
    /// `selected_index` is an index into the *filtered* list, so a refresh that
    /// shrinks the list — or a search that narrows it — can leave it past the
    /// end. Without this, the detail pane rendered "Select an item" while rows
    /// were still highlighted.
    fn clamp_selection(&mut self) {
        let len = self.filtered_item_count();
        if len == 0 {
            self.selected_index = 0;
            self.selected_log_service = 0;
        } else {
            if self.selected_index >= len {
                self.selected_index = len - 1;
            }
            if self.selected_log_service >= self.logs_services.len() {
                self.selected_log_service = self.logs_services.len().saturating_sub(1);
            }
        }
    }

    /// Reload the active tab, awaiting the daemon.
    ///
    /// Used by the manual `r` key and on tab switches, where blocking is fine.
    /// The auto-refresh loop uses [`fetch_snapshot`] + [`App::apply`] instead so
    /// the UI keeps drawing while the request is in flight.
    pub async fn refresh(&mut self, client: Option<&DockerClient>) -> anyhow::Result<()> {
        let owned = client.cloned();
        let state = self.state;
        let snapshot = fetch_snapshot(owned, state).await?;
        self.apply(snapshot);
        Ok(())
    }

    /// Adopt a fetched snapshot and record when it happened.
    pub fn apply(&mut self, snapshot: Snapshot) {
        self.last_refresh = Instant::now();
        match snapshot {
            Snapshot::Services(rows) => {
                self.total_items = rows.len();
                self.services = rows;
            }
            Snapshot::Nodes(rows) => {
                self.total_items = rows.len();
                self.nodes = rows;
            }
            Snapshot::Networks(rows) => {
                self.total_items = rows.len();
                self.networks = rows;
            }
            Snapshot::Ports(rows) => {
                self.total_items = rows.len();
                self.ports = rows;
            }
            Snapshot::Secrets(rows) => {
                self.total_items = rows.len();
                self.secrets = rows;
            }
            Snapshot::Tasks(rows) => {
                self.total_items = rows.len();
                self.tasks = rows;
            }
            Snapshot::Events(rows) => {
                self.total_items = rows.len();
                self.events = rows;
            }
            Snapshot::Logs { services, logs } => {
                self.total_items = services.len();
                self.logs_services = services;
                self.current_logs = logs;
            }
            Snapshot::Unchanged => {}
        }
        // The selection may now be past the end if the list shrank.
        self.clamp_selection();
    }

    async fn fetch_selected_logs(&mut self, client: Option<&DockerClient>) -> anyhow::Result<()> {
        let Some(client) = client else {
            self.current_logs = String::new();
            return Ok(());
        };
        if self.logs_services.is_empty() {
            self.current_logs = String::new();
            return Ok(());
        }
        let index = self.selected_log_service.min(self.logs_services.len() - 1);
        let name = self.logs_services[index].0.clone();
        self.current_logs = fetch_logs(client, &name).await?;
        Ok(())
    }
}

/// One tab's worth of freshly fetched data.
///
/// Returned by value so the auto-refresh loop can fetch on a spawned task while
/// `App` stays confined to the UI task.
pub enum Snapshot {
    Services(Vec<crate::models::service::ServiceRow>),
    Nodes(Vec<crate::models::node::NodeRow>),
    Networks(Vec<crate::models::network::NetworkRow>),
    Ports(Vec<crate::models::port::ServicePortInfo>),
    Secrets(Vec<crate::models::secret::SecretRow>),
    Tasks(Vec<crate::models::task::TaskRow>),
    Events(Vec<EventEntry>),
    Logs {
        services: Vec<(String, String)>,
        logs: String,
    },
    /// No client: nothing to fetch.
    Unchanged,
}

/// Fetch one tab's data.
///
/// Every row comes from the shared builders in `cli::rows.rs`, the same ones
/// `swarmctl get` uses. This previously re-implemented them inline and had
/// already drifted in five ways the CLI had fixed and regression-tested
/// against:
///
/// * service replicas were rendered `desired/desired`, so a 1-of-3 service
///   displayed `3/3`;
/// * node hostname read `spec.name` only, so nodes that joined before a
///   hostname was recorded showed blank;
/// * node status defaulted to `READY`, presenting an unreachable node as
///   healthy;
/// * task ports were always empty and the node column showed raw 25-char IDs
///   rather than hostnames;
/// * labels were joined inline and unsorted, instead of through
///   `selectors::format_labels`.
pub async fn fetch_snapshot(
    client: Option<DockerClient>,
    state: AppState,
) -> anyhow::Result<Snapshot> {
    let Some(client) = client else {
        return Ok(Snapshot::Unchanged);
    };
    let docker = client.inner();

    Ok(match state {
        AppState::Services => {
            let raw = crate::api::service::list_services(docker).await?;
            Snapshot::Services(crate::cli::rows::service_rows(docker, &raw).await?)
        }
        AppState::Nodes => {
            let raw = crate::api::node::list_nodes(docker).await?;
            Snapshot::Nodes(crate::cli::rows::node_rows(&raw))
        }
        AppState::Networks => {
            let raw = crate::api::network::list_networks(docker).await?;
            Snapshot::Networks(crate::cli::rows::network_rows(&raw))
        }
        AppState::Ports => Snapshot::Ports(crate::api::port::list_service_ports(docker).await?),
        AppState::Secrets => {
            let raw = crate::api::secret::list_secrets(docker).await?;
            Snapshot::Secrets(crate::cli::rows::secret_rows(&raw))
        }
        AppState::Tasks => {
            let raw = crate::api::task::list_tasks(docker).await?;
            let hostnames = crate::cli::rows::node_hostnames(docker).await;
            Snapshot::Tasks(crate::cli::rows::task_rows_with_nodes(&raw, &hostnames))
        }
        AppState::Events => Snapshot::Events(fetch_events(&client).await?),
        AppState::Logs => {
            let raw = crate::api::service::list_services(docker).await?;
            let services: Vec<(String, String)> = raw
                .iter()
                .filter_map(|s| {
                    let name = s.spec.as_ref().and_then(|sp| sp.name.clone());
                    let id = s.id.clone().unwrap_or_default();
                    name.map(|n| (n, id))
                })
                .collect();
            let logs = match services.first() {
                Some((name, _)) => fetch_logs(&client, name).await?,
                None => String::new(),
            };
            Snapshot::Logs { services, logs }
        }
    })
}

/// Recent swarm events, newest window capped at 200 entries.
async fn fetch_events(client: &DockerClient) -> anyhow::Result<Vec<EventEntry>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let since = now.saturating_sub(300);

    let options = bollard::query_parameters::EventsOptions {
        since: Some(since.to_string()),
        until: Some(now.to_string()),
        filters: None,
    };

    let mut stream = client.inner().events(Some(options));
    let mut events = Vec::new();
    while events.len() < 200 {
        let Some(result) = stream.next().await else {
            break;
        };
        let Ok(ev) = result else {
            continue;
        };
        let ts = ev.time.unwrap_or(0);
        let kind = ev.typ.map(|t| format!("{:?}", t)).unwrap_or_default();
        let action = ev.action.unwrap_or_default();
        let id = ev
            .actor
            .as_ref()
            .and_then(|a| a.id.clone())
            .unwrap_or_default();
        let time_str = if ts > 0 {
            let secs = ts % 60;
            let mins = (ts / 60) % 60;
            let hours = (ts / 3600) % 24;
            format!("{:02}:{:02}:{:02}", hours, mins, secs)
        } else {
            "??:??:??".to_string()
        };
        events.push(EventEntry {
            time: time_str,
            kind,
            action,
            id,
        });
    }
    Ok(events)
}

/// Tail one service's logs by name.
///
/// Errors are rendered inline rather than propagated, so a bad service name
/// shows in the pane instead of tearing down the TUI.
async fn fetch_logs(client: &DockerClient, name: &str) -> anyhow::Result<String> {
    let options = bollard::query_parameters::LogsOptions {
        follow: false,
        stdout: true,
        stderr: true,
        timestamps: true,
        tail: "200".to_string(),
        ..Default::default()
    };
    let mut stream = client.inner().service_logs(name, Some(options));
    let mut logs = String::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(line) => logs.push_str(&line.to_string()),
            Err(e) => {
                logs.push_str(&format!("[Error: {}]\n", e));
                break;
            }
        }
    }
    Ok(logs)
}

pub async fn run_tui(client: Option<&DockerClient>) -> anyhow::Result<()> {
    let host_info = match client {
        Some(c) => match c.inner().info().await {
            Ok(info) => {
                let name = info.name.unwrap_or_else(|| "docker".to_string());
                format!("docker@{}", name)
            }
            Err(_) => "docker".to_string(),
        },
        None => "docker (disconnected)".to_string(),
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(host_info);
    if let Some(c) = client {
        app.refresh(Some(c)).await?;
    }

    let res = run_app(&mut terminal, &mut app, client).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    client: Option<&DockerClient>,
) -> anyhow::Result<()> {
    let poll_interval = Duration::from_millis(100);

    // An in-flight fetch, so the UI keeps drawing and accepting keys while the
    // daemon is queried. Awaiting the refresh inline meant a slow or remote
    // endpoint froze the dashboard for the whole request — over SSH, seconds.
    let mut pending: Option<tokio::task::JoinHandle<anyhow::Result<Snapshot>>> = None;

    loop {
        let now = Instant::now();

        if pending.is_none()
            && app.auto_refresh
            && now.duration_since(app.last_refresh)
                >= Duration::from_secs(app.auto_refresh_interval)
        {
            let owned = client.cloned();
            let state = app.state;
            pending = Some(tokio::spawn(
                async move { fetch_snapshot(owned, state).await },
            ));
        }

        // Apply a finished fetch. The data comes back by value so `App` is only
        // ever mutated on this task.
        if let Some(handle) = &pending
            && handle.is_finished()
        {
            let handle = pending.take().expect("checked above");
            match handle.await {
                // A failed refresh leaves the last good rows on screen rather
                // than tearing the dashboard down.
                Ok(Ok(snapshot)) => app.apply(snapshot),
                Ok(Err(e)) => log::warn!("dashboard refresh failed: {}", e),
                Err(e) => log::warn!("dashboard refresh task failed: {}", e),
            }
        }

        terminal.draw(|f| ui(f, app))?;

        if event::poll(poll_interval)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            if app.search_mode {
                match key.code {
                    KeyCode::Esc => {
                        app.search_mode = false;
                        app.search_query.clear();
                    }
                    KeyCode::Enter => {
                        app.search_mode = false;
                    }
                    KeyCode::Backspace => {
                        app.search_query.pop();
                        app.selected_index = 0;
                    }
                    KeyCode::Char(c) => {
                        app.search_query.push(c);
                        app.selected_index = 0;
                    }
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        if app.detail_open {
                            app.detail_open = false;
                        } else {
                            return Ok(());
                        }
                    }
                    KeyCode::Char('r') => {
                        app.refresh(client).await?;
                    }
                    KeyCode::Char('/') => {
                        app.search_mode = true;
                        app.search_query.clear();
                    }
                    KeyCode::Char('a') => {
                        app.auto_refresh = !app.auto_refresh;
                    }
                    KeyCode::Char('+') | KeyCode::Char('=') => {
                        app.auto_refresh_interval = match app.auto_refresh_interval {
                            2 => 5,
                            5 => 10,
                            10 => 30,
                            30 => 60,
                            _ => 60,
                        };
                    }
                    KeyCode::Char('-') => {
                        app.auto_refresh_interval = match app.auto_refresh_interval {
                            60 => 30,
                            30 => 10,
                            10 => 5,
                            5 => 2,
                            _ => 2,
                        };
                    }
                    KeyCode::Char('d') | KeyCode::Enter => {
                        app.detail_open = !app.detail_open;
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        let was_logs = app.state == AppState::Logs;
                        app.move_selection_down();
                        // The Logs pane shows one service's output, so moving
                        // the cursor has to reload them; otherwise the pane
                        // kept showing the previously selected service.
                        if was_logs {
                            app.fetch_selected_logs(client).await?;
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        let was_logs = app.state == AppState::Logs;
                        app.move_selection_up();
                        if was_logs {
                            app.fetch_selected_logs(client).await?;
                        }
                    }
                    KeyCode::Tab => {
                        app.state = app.state.next();
                        app.selected_index = 0;
                        app.detail_open = false;
                        app.refresh(client).await?;
                    }
                    KeyCode::BackTab => {
                        app.state = app.state.prev();
                        app.selected_index = 0;
                        app.detail_open = false;
                        app.refresh(client).await?;
                    }
                    KeyCode::Char('1') => {
                        app.state = AppState::Services;
                        app.selected_index = 0;
                        app.detail_open = false;
                        app.refresh(client).await?;
                    }
                    KeyCode::Char('2') => {
                        app.state = AppState::Nodes;
                        app.selected_index = 0;
                        app.detail_open = false;
                        app.refresh(client).await?;
                    }
                    KeyCode::Char('3') => {
                        app.state = AppState::Networks;
                        app.selected_index = 0;
                        app.detail_open = false;
                        app.refresh(client).await?;
                    }
                    KeyCode::Char('4') => {
                        app.state = AppState::Ports;
                        app.selected_index = 0;
                        app.detail_open = false;
                        app.refresh(client).await?;
                    }
                    KeyCode::Char('5') => {
                        app.state = AppState::Secrets;
                        app.selected_index = 0;
                        app.detail_open = false;
                        app.refresh(client).await?;
                    }
                    KeyCode::Char('6') => {
                        app.state = AppState::Tasks;
                        app.selected_index = 0;
                        app.detail_open = false;
                        app.refresh(client).await?;
                    }
                    KeyCode::Char('7') => {
                        app.state = AppState::Events;
                        app.selected_index = 0;
                        app.detail_open = false;
                        app.refresh(client).await?;
                    }
                    KeyCode::Char('8') => {
                        app.state = AppState::Logs;
                        app.selected_index = 0;
                        app.detail_open = false;
                        app.refresh(client).await?;
                    }
                    _ => {}
                }
            }
        }
    }
}

fn ui(f: &mut ratatui::Frame, app: &App) {
    let size = f.area();

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Length(1), // tabs
            Constraint::Min(0),    // content
            Constraint::Length(1), // footer
        ])
        .split(size);

    render_header(f, app, main_chunks[0]);
    render_tabs(f, app, main_chunks[1]);
    render_content(f, app, main_chunks[2]);
    render_footer(f, app, main_chunks[3]);
}

fn render_header(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let auto_status = if app.auto_refresh {
        format!("auto:{}s", app.auto_refresh_interval)
    } else {
        "auto:off".to_string()
    };

    let header_text = Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(
            "● ",
            Style::default()
                .fg(COLOR_SUCCESS)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "swarmctl",
            Style::default()
                .fg(COLOR_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  │  ", Style::default().fg(COLOR_DIM)),
        Span::styled(&app.host_info, Style::default().fg(Color::White)),
        Span::styled("  │  ", Style::default().fg(COLOR_DIM)),
        Span::styled(
            auto_status,
            Style::default().fg(if app.auto_refresh {
                COLOR_SUCCESS
            } else {
                COLOR_DIM
            }),
        ),
        Span::styled("  │  ", Style::default().fg(COLOR_DIM)),
        Span::styled(
            app.state.label().to_string(),
            Style::default()
                .fg(COLOR_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({})", app.filtered_item_count()),
            Style::default().fg(COLOR_DIM),
        ),
    ]);

    let header =
        Paragraph::new(header_text).style(Style::default().bg(COLOR_HEADER_BG).fg(Color::White));
    f.render_widget(header, area);
}

fn render_tabs(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let tab_labels: Vec<Line> = AppState::all()
        .iter()
        .map(|state| {
            let is_active = *state == app.state;
            let label = format!("[{}]{}", state.key(), state.label());
            let style = if is_active {
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(COLOR_DIM)
            };
            Line::from(Span::styled(label, style))
        })
        .collect();

    let tabs = Tabs::new(tab_labels)
        .select(app.state.index())
        .style(Style::default().fg(COLOR_DIM))
        .highlight_style(
            Style::default()
                .fg(COLOR_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled(" │ ", Style::default().fg(COLOR_DIM)));

    let tab_block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(COLOR_DIM));

    let tabs_widget = tabs.block(tab_block);
    f.render_widget(tabs_widget, area);
}

fn render_content(f: &mut ratatui::Frame, app: &App, area: Rect) {
    if app.detail_open {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);

        render_main_list(f, app, chunks[0]);
        render_detail_pane(f, app, chunks[1]);
    } else {
        render_main_list(f, app, area);
    }
}

fn render_main_list(f: &mut ratatui::Frame, app: &App, area: Rect) {
    match app.state {
        AppState::Services => render_services_table(f, app, area),
        AppState::Nodes => render_nodes_table(f, app, area),
        AppState::Networks => render_networks_table(f, app, area),
        AppState::Ports => render_ports_table(f, app, area),
        AppState::Secrets => render_secrets_table(f, app, area),
        AppState::Tasks => render_tasks_table(f, app, area),
        AppState::Events => render_events_list(f, app, area),
        AppState::Logs => render_logs(f, app, area),
    }
}

fn make_header_row<'a>(cols: &[&'a str]) -> Row<'a> {
    Row::new(cols.iter().map(|c| {
        Cell::from(*c).style(
            Style::default()
                .fg(COLOR_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )
    }))
}

fn selected_style(is_selected: bool) -> Style {
    if is_selected {
        Style::default()
            .bg(COLOR_SELECTION_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

fn render_services_table(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered = app.filtered(&app.services, |s| &s.name);

    let header = make_header_row(&["", "Name", "Mode", "Replicas", "Image"]);

    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_selected = i == app.selected_index;
            // On the replicas column ("3/3", "1/3", "N/A"). Passing it to
            // `status_icon` matched no keyword, so the dot was always dim;
            // parse the running count so a degraded service is visible.
            let (icon, color) = App::replica_icon(&s.replicas);
            let style = selected_style(is_selected);

            Row::new(vec![
                Cell::from(Span::styled(icon, Style::default().fg(color))),
                Cell::from(Span::styled(&s.name, style)),
                Cell::from(Span::styled(&s.mode, style)),
                Cell::from(Span::styled(&s.replicas, style)),
                Cell::from(Span::styled(truncate(&s.image, 40), style.fg(COLOR_DIM))),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(22),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_DIM))
            .title(format!(" Services ({}) ", app.filtered_item_count()))
            .title_style(
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
    )
    .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_widget(table, area);
}

fn render_nodes_table(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered = app.filtered(&app.nodes, |n| &n.hostname);

    let header = make_header_row(&["", "Hostname", "Status", "Availability", "Role"]);

    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let is_selected = i == app.selected_index;
            let (icon, color) = App::status_icon(&n.status);
            let avail_color = match n.availability.to_lowercase().as_str() {
                "active" => COLOR_SUCCESS,
                "pause" | "paused" => COLOR_WARNING,
                "drain" | "draining" => COLOR_ERROR,
                _ => COLOR_DIM,
            };
            let style = selected_style(is_selected);

            Row::new(vec![
                Cell::from(Span::styled(icon, Style::default().fg(color))),
                Cell::from(Span::styled(&n.hostname, style)),
                Cell::from(Span::styled(&n.status, style.fg(color))),
                Cell::from(Span::styled(&n.availability, style.fg(avail_color))),
                Cell::from(Span::styled(&n.manager, style.fg(COLOR_DIM))),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(22),
            Constraint::Length(12),
            Constraint::Length(14),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_DIM))
            .title(format!(" Nodes ({}) ", app.filtered_item_count()))
            .title_style(
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(table, area);
}

fn render_networks_table(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered = app.filtered(&app.networks, |n| &n.name);

    let header = make_header_row(&["", "Name", "Driver", "Scope", "Internal"]);

    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let is_selected = i == app.selected_index;
            let style = selected_style(is_selected);

            Row::new(vec![
                Cell::from(Span::styled("●", Style::default().fg(COLOR_SUCCESS))),
                Cell::from(Span::styled(&n.name, style)),
                Cell::from(Span::styled(&n.driver, style)),
                Cell::from(Span::styled(&n.scope, style.fg(COLOR_DIM))),
                Cell::from(Span::styled(&n.internal, style.fg(COLOR_DIM))),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(22),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_DIM))
            .title(format!(" Networks ({}) ", app.filtered_item_count()))
            .title_style(
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(table, area);
}

fn render_ports_table(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered = app.filtered(&app.ports, |p| &p.service_name);

    let header = make_header_row(&["", "Service", "Published", "Target", "Protocol"]);

    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let is_selected = i == app.selected_index;
            let style = selected_style(is_selected);

            Row::new(vec![
                Cell::from(Span::styled("●", Style::default().fg(COLOR_SUCCESS))),
                Cell::from(Span::styled(&p.service_name, style)),
                Cell::from(Span::styled(&p.published_port, style)),
                Cell::from(Span::styled(&p.target_port, style)),
                Cell::from(Span::styled(
                    &p.protocol,
                    style.fg(if p.protocol == "tcp" {
                        COLOR_SUCCESS
                    } else {
                        COLOR_WARNING
                    }),
                )),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(22),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_DIM))
            .title(format!(" Ports ({}) ", app.filtered_item_count()))
            .title_style(
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(table, area);
}

fn render_secrets_table(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered = app.filtered(&app.secrets, |s| &s.name);

    let header = make_header_row(&["", "Name", "Created"]);

    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_selected = i == app.selected_index;
            let style = selected_style(is_selected);

            Row::new(vec![
                Cell::from(Span::styled("●", Style::default().fg(COLOR_WARNING))),
                Cell::from(Span::styled(&s.name, style)),
                Cell::from(Span::styled(&s.created_at, style.fg(COLOR_DIM))),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(22),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_DIM))
            .title(format!(" Secrets ({}) ", app.filtered_item_count()))
            .title_style(
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(table, area);
}

fn render_tasks_table(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered = app.filtered(&app.tasks, |t| &t.name);

    let header = make_header_row(&["", "Service", "State", "Node", "Image"]);

    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let is_selected = i == app.selected_index;
            let (icon, color) = App::status_icon(&t.current_state);
            let style = selected_style(is_selected);

            Row::new(vec![
                Cell::from(Span::styled(icon, Style::default().fg(color))),
                Cell::from(Span::styled(&t.name, style)),
                Cell::from(Span::styled(&t.current_state, style.fg(color))),
                Cell::from(Span::styled(truncate(&t.node, 16), style.fg(COLOR_DIM))),
                Cell::from(Span::styled(truncate(&t.image, 30), style.fg(COLOR_DIM))),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(22),
            Constraint::Length(14),
            Constraint::Length(18),
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_DIM))
            .title(format!(" Tasks ({}) ", app.filtered_item_count()))
            .title_style(
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(table, area);
}

fn render_events_list(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered = app.filtered_multi(&app.events, |e| [&e.action, &e.id]);

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let is_selected = i == app.selected_index;
            let style = selected_style(is_selected);
            let kind_color = match e.kind.to_lowercase().as_str() {
                "service" => COLOR_SUCCESS,
                "container" => COLOR_PRIMARY,
                "network" => COLOR_WARNING,
                "node" => COLOR_SUCCESS,
                _ => COLOR_DIM,
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("[{}] ", e.time), style.fg(COLOR_DIM)),
                Span::styled(format!("{:8}", e.kind), style.fg(kind_color)),
                Span::styled(format!("{:10}", e.action), style),
                Span::styled(truncate(&e.id, 16), style.fg(COLOR_DIM)),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_DIM))
            .title(format!(" Events ({}) ", app.filtered_item_count()))
            .title_style(
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(list, area);
}

fn render_logs(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(0)])
        .split(area);

    let service_items: Vec<ListItem> = if app.logs_services.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  No services",
            Style::default().fg(COLOR_DIM),
        )))]
    } else {
        app.logs_services
            .iter()
            .enumerate()
            .map(|(i, (name, _))| {
                let is_selected = i == app.selected_log_service;
                let prefix = if is_selected { "▸ " } else { "  " };
                let style = if is_selected {
                    Style::default()
                        .fg(COLOR_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(Span::styled(
                    format!("{}{}", prefix, truncate(name, 20)),
                    style,
                )))
            })
            .collect()
    };

    let services_list = List::new(service_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_DIM))
            .title(" Services ")
            .title_style(
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(services_list, chunks[0]);

    let log_lines: Vec<ListItem> = if app.current_logs.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  Select a service and press Enter",
            Style::default().fg(COLOR_DIM),
        )))]
    } else {
        app.current_logs
            .lines()
            .map(|line| {
                let style = if line.contains("Error") || line.contains("error") {
                    Style::default().fg(COLOR_ERROR)
                } else if line.contains("WARN") || line.contains("warn") {
                    Style::default().fg(COLOR_WARNING)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(Span::styled(line.to_string(), style)))
            })
            .collect()
    };

    let log_list = List::new(log_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_DIM))
            .title(format!(
                " Logs: {} ",
                app.logs_services
                    .get(app.selected_log_service)
                    .map(|(n, _)| n.as_str())
                    .unwrap_or("-")
            ))
            .title_style(
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(log_list, chunks[1]);
}

/// Detail pane for the highlighted row.
///
/// Reads through [`App::filtered`] rather than the raw vector: the tables
/// highlight a row at `selected_index` *within the filtered list*, so indexing
/// the unfiltered list showed the wrong resource as soon as a search was active.
/// This was the single largest correctness bug in the TUI.
fn render_detail_pane(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    match app.state {
        AppState::Services => {
            if let Some(s) = app
                .filtered(&app.services, |s| &s.name)
                .get(app.selected_index)
            {
                lines.push(Line::from(Span::styled(
                    " Service Details ",
                    Style::default()
                        .fg(COLOR_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(detail_line("Name", s.name.clone()));
                lines.push(detail_line("ID", truncate(&s.id, 16)));
                lines.push(detail_line("Image", s.image.clone()));
                lines.push(detail_line("Mode", s.mode.clone()));
                lines.push(detail_line("Replicas", s.replicas.clone()));
                if !s.labels.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        " Labels",
                        Style::default()
                            .fg(COLOR_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    )));
                    for label in s.labels.split(',') {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", label.trim()),
                            Style::default().fg(COLOR_DIM),
                        )));
                    }
                }
            }
        }
        AppState::Nodes => {
            if let Some(n) = app
                .filtered(&app.nodes, |n| &n.hostname)
                .get(app.selected_index)
            {
                lines.push(Line::from(Span::styled(
                    " Node Details ",
                    Style::default()
                        .fg(COLOR_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(detail_line("Hostname", n.hostname.clone()));
                lines.push(detail_line("ID", truncate(&n.id, 16)));
                lines.push(detail_line("Status", n.status.clone()));
                lines.push(detail_line("Availability", n.availability.clone()));
                lines.push(detail_line("Role", n.manager.clone()));
                if !n.labels.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        " Labels",
                        Style::default()
                            .fg(COLOR_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    )));
                    for label in n.labels.split(',') {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", label.trim()),
                            Style::default().fg(COLOR_DIM),
                        )));
                    }
                }
            }
        }
        AppState::Tasks => {
            if let Some(t) = app
                .filtered(&app.tasks, |t| &t.name)
                .get(app.selected_index)
            {
                lines.push(Line::from(Span::styled(
                    " Task Details ",
                    Style::default()
                        .fg(COLOR_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(detail_line("Name", t.name.clone()));
                lines.push(detail_line("ID", truncate(&t.id, 16)));
                lines.push(detail_line("Image", t.image.clone()));
                lines.push(detail_line("Desired", t.desired_state.clone()));
                lines.push(detail_line("Current", t.current_state.clone()));
                lines.push(detail_line("Node", t.node.clone()));
            }
        }
        AppState::Networks => {
            if let Some(n) = app
                .filtered(&app.networks, |n| &n.name)
                .get(app.selected_index)
            {
                lines.push(Line::from(Span::styled(
                    " Network Details ",
                    Style::default()
                        .fg(COLOR_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(detail_line("Name", n.name.clone()));
                lines.push(detail_line("ID", truncate(&n.id, 16)));
                lines.push(detail_line("Driver", n.driver.clone()));
                lines.push(detail_line("Scope", n.scope.clone()));
                lines.push(detail_line("Internal", n.internal.clone()));
            }
        }
        AppState::Secrets => {
            if let Some(s) = app
                .filtered(&app.secrets, |s| &s.name)
                .get(app.selected_index)
            {
                lines.push(Line::from(Span::styled(
                    " Secret Details ",
                    Style::default()
                        .fg(COLOR_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(detail_line("Name", s.name.clone()));
                lines.push(detail_line("ID", truncate(&s.id, 16)));
                lines.push(detail_line("Created", s.created_at.clone()));
                if !s.labels.is_empty() {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        " Labels",
                        Style::default()
                            .fg(COLOR_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    )));
                    for label in s.labels.split(',') {
                        lines.push(Line::from(Span::styled(
                            format!("  {}", label.trim()),
                            Style::default().fg(COLOR_DIM),
                        )));
                    }
                }
            }
        }
        AppState::Ports => {
            if let Some(p) = app
                .filtered(&app.ports, |p| &p.service_name)
                .get(app.selected_index)
            {
                lines.push(Line::from(Span::styled(
                    " Port Details ",
                    Style::default()
                        .fg(COLOR_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(detail_line("Service", p.service_name.clone()));
                lines.push(detail_line("Published", p.published_port.clone()));
                lines.push(detail_line("Target", p.target_port.clone()));
                lines.push(detail_line("Protocol", p.protocol.clone()));
                lines.push(detail_line("Mode", p.publish_mode.clone()));
            }
        }
        _ => {
            lines.push(Line::from(Span::styled(
                " No details available",
                Style::default().fg(COLOR_DIM),
            )));
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            " Select an item",
            Style::default().fg(COLOR_DIM),
        )));
    }

    let detail = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_DIM))
            .title(" Details ")
            .title_style(
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(detail, area);
}

fn render_footer(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let footer_text = if app.search_mode {
        Line::from(vec![
            Span::styled(
                " /",
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}█", app.search_query),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                "  Esc to cancel  Enter to confirm",
                Style::default().fg(COLOR_DIM),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                " /",
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Search  ", Style::default().fg(COLOR_DIM)),
            Span::styled(
                "a",
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if app.auto_refresh { "On " } else { "Off" },
                Style::default().fg(if app.auto_refresh {
                    COLOR_SUCCESS
                } else {
                    COLOR_DIM
                }),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                "+/-",
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}s  ", app.auto_refresh_interval),
                Style::default().fg(COLOR_DIM),
            ),
            Span::styled(
                "d",
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Detail  ", Style::default().fg(COLOR_DIM)),
            Span::styled(
                "j/k",
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Nav  ", Style::default().fg(COLOR_DIM)),
            Span::styled(
                "Tab",
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Switch  ", Style::default().fg(COLOR_DIM)),
            Span::styled(
                "r",
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Refresh  ", Style::default().fg(COLOR_DIM)),
            Span::styled(
                "q",
                Style::default()
                    .fg(COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Quit", Style::default().fg(COLOR_DIM)),
        ])
    };

    let footer =
        Paragraph::new(footer_text).style(Style::default().bg(COLOR_HEADER_BG).fg(Color::White));
    f.render_widget(footer, area);
}

fn detail_line(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {:12}", label), Style::default().fg(COLOR_DIM)),
        Span::styled(value, Style::default().fg(Color::White)),
    ])
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App::new("test".into())
    }

    fn services(names: &[&str]) -> Vec<crate::models::service::ServiceRow> {
        names
            .iter()
            .map(|n| crate::models::service::ServiceRow {
                id: format!("{n}-id"),
                name: n.to_string(),
                mode: "replicated".into(),
                replicas: "1/1".into(),
                image: "nginx".into(),
                labels: String::new(),
            })
            .collect()
    }

    /// A node with no status must not be presented as healthy. The TUI used to
    /// default a missing state to `READY`, reintroducing the bug
    /// `cli::rows::node_rows` documents and regression-tests against.
    #[test]
    fn node_rows_reach_the_tui_with_unknown_status() {
        let rows = crate::cli::rows::node_rows(&[bollard::models::Node {
            id: Some("n1".into()),
            status: None,
            ..Default::default()
        }]);
        assert_eq!(rows[0].status, "<unknown>");
    }

    /// Service replicas come from a task listing now, so a degraded service can
    /// be shown accurately. The TUI previously computed `desired/desired`.
    #[test]
    fn replica_icon_distinguishes_healthy_from_degraded() {
        let color = |c: ratatui::style::Color| c;
        assert_eq!(color(App::replica_icon("3/3").1), COLOR_SUCCESS);
        assert_eq!(color(App::replica_icon("1/3").1), COLOR_ERROR);
        assert_eq!(color(App::replica_icon("0/3").1), COLOR_ERROR);
        // A global service has no replica count.
        assert_eq!(color(App::replica_icon("N/A").1), COLOR_DIM);
        // Scaled to zero is neither running nor failing.
        assert_eq!(color(App::replica_icon("0/0").1), COLOR_DIM);
        assert_eq!(color(App::replica_icon("").1), COLOR_DIM);
    }

    /// `j`/`k` moved `selected_index`, which the Logs tab never reads, and
    /// `selected_log_service` was only ever assigned 0 — so the Logs pane could
    /// not be navigated at all.
    #[test]
    fn moving_the_cursor_advances_the_log_selection() {
        let mut a = app();
        a.state = AppState::Logs;
        a.logs_services = vec![("web".into(), "1".into()), ("api".into(), "2".into())];

        a.move_selection_down();
        assert_eq!(
            a.selected_log_service, 1,
            "down should advance the log cursor"
        );

        a.move_selection_down();
        assert_eq!(a.selected_log_service, 1, "must clamp at the end");

        a.move_selection_up();
        assert_eq!(a.selected_log_service, 0, "up should go back");
        a.move_selection_up();
        assert_eq!(a.selected_log_service, 0, "must not underflow");
    }

    #[test]
    fn moving_the_cursor_still_works_on_row_tabs() {
        let mut a = app();
        a.services = services(&["a", "b", "c"]);
        a.move_selection_down();
        assert_eq!(a.selected_index, 1);
        a.move_selection_down();
        assert_eq!(a.selected_index, 2);
        a.move_selection_down();
        assert_eq!(a.selected_index, 2, "must clamp at the end");
        a.move_selection_up();
        assert_eq!(a.selected_index, 1);
    }

    /// The cursor is an index into the *filtered* list, so a search that
    /// narrows the list can leave it past the end.
    #[test]
    fn clamp_selection_pulls_the_cursor_back_into_range() {
        let mut a = app();
        a.services = services(&["alpha", "beta", "gamma"]);
        a.selected_index = 2;
        a.clamp_selection();
        assert_eq!(a.selected_index, 2);

        a.search_query = "alpha".into();
        a.clamp_selection();
        assert!(
            a.selected_index <= a.filtered_item_count().saturating_sub(1),
            "selection {} out of range for {} visible rows",
            a.selected_index,
            a.filtered_item_count()
        );
    }

    #[test]
    fn clamp_selection_resets_when_nothing_matches() {
        let mut a = app();
        a.services = services(&["alpha"]);
        a.selected_index = 0;
        a.search_query = "zzz".into();
        a.clamp_selection();
        assert_eq!(a.selected_index, 0);
        assert_eq!(a.filtered_item_count(), 0);
    }

    /// The header count used to return the unfiltered total on the Ports,
    /// Events and Logs tabs even though those renderers do filter.
    #[test]
    fn filtered_count_matches_the_rendered_rows_on_every_tab() {
        let mut a = app();

        a.state = AppState::Services;
        a.services = services(&["alpha", "beta"]);
        a.search_query = "alpha".into();
        assert_eq!(a.filtered_item_count(), 1);

        a.state = AppState::Nodes;
        a.nodes = vec![
            crate::models::node::NodeRow {
                id: "1".into(),
                hostname: "alpha-1".into(),
                status: "ready".into(),
                availability: "active".into(),
                manager: "-".into(),
                labels: String::new(),
            },
            crate::models::node::NodeRow {
                id: "2".into(),
                hostname: "beta-1".into(),
                status: "ready".into(),
                availability: "active".into(),
                manager: "-".into(),
                labels: String::new(),
            },
        ];
        assert_eq!(a.filtered_item_count(), 1, "nodes filter on hostname");

        a.state = AppState::Ports;
        a.ports = vec![
            crate::models::port::ServicePortInfo {
                service_name: "alpha".into(),
                published_port: "8080".into(),
                target_port: "80".into(),
                protocol: "tcp".into(),
                publish_mode: "ingress".into(),
            },
            crate::models::port::ServicePortInfo {
                service_name: "beta".into(),
                published_port: "9090".into(),
                target_port: "90".into(),
                protocol: "tcp".into(),
                publish_mode: "ingress".into(),
            },
        ];
        assert_eq!(a.filtered_item_count(), 1, "ports filter on service name");

        a.state = AppState::Events;
        a.events = vec![
            EventEntry {
                time: "00:00:00".into(),
                kind: "service".into(),
                action: "create".into(),
                id: "alpha".into(),
            },
            EventEntry {
                time: "00:00:01".into(),
                kind: "service".into(),
                action: "remove".into(),
                id: "beta".into(),
            },
        ];
        assert_eq!(a.filtered_item_count(), 1, "events match action or id");
        // Also matched by action.
        a.search_query = "remove".into();
        assert_eq!(a.filtered_item_count(), 1);
    }

    #[test]
    fn an_empty_search_matches_everything() {
        let mut a = app();
        a.services = services(&["alpha", "beta"]);
        a.search_query = String::new();
        assert_eq!(a.filtered_item_count(), 2);
        assert_eq!(a.filtered(&a.services, |s| &s.name).len(), 2);
    }

    #[test]
    fn search_is_case_insensitive() {
        let mut a = app();
        a.services = services(&["Web"]);
        a.search_query = "web".into();
        assert_eq!(a.filtered_item_count(), 1);
    }

    #[test]
    fn applying_a_snapshot_updates_the_rows_and_count() {
        let mut a = app();
        a.apply(Snapshot::Services(services(&["a", "b", "c"])));
        assert_eq!(a.services.len(), 3);
        assert_eq!(a.total_items, 3);
        assert_eq!(a.filtered_item_count(), 3);

        a.apply(Snapshot::Unchanged);
        assert_eq!(a.services.len(), 3, "Unchanged must not clear the rows");
    }

    #[test]
    fn status_icon_maps_known_states() {
        assert_eq!(App::status_icon("running").1, COLOR_SUCCESS);
        assert_eq!(App::status_icon("down").1, COLOR_ERROR);
        assert_eq!(App::status_icon("pending").1, COLOR_WARNING);
        // An unrecognised state is dim, not an error.
        assert_eq!(App::status_icon("<unknown>").1, COLOR_DIM);
    }

    #[test]
    fn app_state_cycles_through_every_tab() {
        let states = AppState::all();
        assert!(states.len() >= 8);
        // Every state must have a label and a distinct hotkey.
        let mut keys: Vec<&str> = Vec::new();
        for s in states {
            assert!(!s.label().is_empty());
            keys.push(s.key());
        }
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), states.len(), "hotkeys must be unique");

        // `next` wraps around the whole list, and `prev` walks it back.
        let mut seen = AppState::Services;
        for _ in 0..states.len() - 1 {
            seen = seen.next();
        }
        assert_eq!(seen, AppState::Logs);
        assert_eq!(seen.next(), AppState::Services, "next must wrap");
        assert_eq!(AppState::Services.prev(), AppState::Logs, "prev must wrap");
    }
}
