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

#[derive(Clone, Copy, PartialEq)]
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
            "failed" | "error" | "down" | "orphaned" | "remove" => {
                ("●".into(), COLOR_ERROR)
            }
            "paused" | "drain" | "draining" | "halted" => ("⏸".into(), COLOR_DIM),
            "complete" | "shutdown" => ("✓".into(), COLOR_DIM),
            _ => ("●".into(), COLOR_DIM),
        }
    }

    fn filtered_item_count(&self) -> usize {
        match self.state {
            AppState::Services => {
                if self.search_query.is_empty() {
                    self.services.len()
                } else {
                    self.services
                        .iter()
                        .filter(|s| s.name.to_lowercase().contains(&self.search_query.to_lowercase()))
                        .count()
                }
            }
            AppState::Nodes => {
                if self.search_query.is_empty() {
                    self.nodes.len()
                } else {
                    self.nodes
                        .iter()
                        .filter(|n| n.hostname.to_lowercase().contains(&self.search_query.to_lowercase()))
                        .count()
                }
            }
            AppState::Networks => {
                if self.search_query.is_empty() {
                    self.networks.len()
                } else {
                    self.networks
                        .iter()
                        .filter(|n| n.name.to_lowercase().contains(&self.search_query.to_lowercase()))
                        .count()
                }
            }
            AppState::Secrets => {
                if self.search_query.is_empty() {
                    self.secrets.len()
                } else {
                    self.secrets
                        .iter()
                        .filter(|s| s.name.to_lowercase().contains(&self.search_query.to_lowercase()))
                        .count()
                }
            }
            AppState::Tasks => {
                if self.search_query.is_empty() {
                    self.tasks.len()
                } else {
                    self.tasks
                        .iter()
                        .filter(|t| t.name.to_lowercase().contains(&self.search_query.to_lowercase()))
                        .count()
                }
            }
            _ => self.total_items,
        }
    }

    pub async fn refresh(&mut self, client: &DockerClient) -> anyhow::Result<()> {
        self.last_refresh = Instant::now();
        match self.state {
            AppState::Services => {
                let raw = crate::api::service::list_services(client.inner()).await?;
                self.services = raw
                    .into_iter()
                    .map(|s| {
                        let spec = s.spec.unwrap_or_default();
                        let name = spec.name.unwrap_or_default();
                        let image = spec
                            .task_template
                            .and_then(|t| t.container_spec)
                            .and_then(|c| c.image)
                            .unwrap_or_default();
                        let (mode, replicas) = match spec.mode {
                            Some(m) if m.replicated.is_some() => {
                                let r = m.replicated.unwrap().replicas.unwrap_or(0);
                                ("replicated".to_string(), format!("{}/{}", r, r))
                            }
                            Some(_) => ("global".to_string(), "N/A".to_string()),
                            None => ("unknown".to_string(), "N/A".to_string()),
                        };
                        let labels = spec
                            .labels
                            .as_ref()
                            .map(|l| {
                                l.iter()
                                    .map(|(k, v)| format!("{}={}", k, v))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                            .unwrap_or_default();
                        crate::models::service::ServiceRow {
                            id: s.id.unwrap_or_default(),
                            name,
                            mode,
                            replicas,
                            image,
                            labels,
                        }
                    })
                    .collect();
                self.total_items = self.services.len();
            }
            AppState::Nodes => {
                let raw = crate::api::node::list_nodes(client.inner()).await?;
                self.nodes = raw
                    .into_iter()
                    .map(|n| {
                        let spec = n.spec.unwrap_or_default();
                        let status = n.status.unwrap_or_default();
                        let manager = n
                            .manager_status
                            .as_ref()
                            .map(|m| {
                                match m
                                    .reachability
                                    .unwrap_or(bollard::models::Reachability::UNKNOWN)
                                {
                                    bollard::models::Reachability::REACHABLE => "Reachable",
                                    bollard::models::Reachability::UNREACHABLE => "Unavailable",
                                    _ => "-",
                                }
                            })
                            .unwrap_or("-");
                        let labels = spec
                            .labels
                            .as_ref()
                            .map(|l| {
                                l.iter()
                                    .map(|(k, v)| format!("{}={}", k, v))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                            .unwrap_or_default();
                        crate::models::node::NodeRow {
                            id: n.id.unwrap_or_default(),
                            hostname: spec.name.unwrap_or_default(),
                            status: status
                                .state
                                .unwrap_or(bollard::models::NodeState::READY)
                                .to_string(),
                            availability: spec
                                .availability
                                .unwrap_or(bollard::models::NodeSpecAvailabilityEnum::ACTIVE)
                                .to_string(),
                            manager: manager.to_string(),
                            labels,
                        }
                    })
                    .collect();
                self.total_items = self.nodes.len();
            }
            AppState::Networks => {
                let raw = crate::api::network::list_networks(client.inner()).await?;
                self.networks = raw
                    .into_iter()
                    .map(|n| {
                        let labels = n
                            .labels
                            .as_ref()
                            .map(|l| {
                                l.iter()
                                    .map(|(k, v)| format!("{}={}", k, v))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                            .unwrap_or_default();
                        crate::models::network::NetworkRow {
                            id: n.id.unwrap_or_default(),
                            name: n.name.unwrap_or_default(),
                            driver: n.driver.unwrap_or_else(|| "unknown".to_string()),
                            scope: n.scope.unwrap_or_else(|| "unknown".to_string()),
                            internal: if n.internal.unwrap_or(false) {
                                "true"
                            } else {
                                "false"
                            }
                            .to_string(),
                            labels,
                        }
                    })
                    .collect();
                self.total_items = self.networks.len();
            }
            AppState::Ports => {
                self.ports = crate::api::port::list_service_ports(client.inner()).await?;
                self.total_items = self.ports.len();
            }
            AppState::Secrets => {
                let raw = crate::api::secret::list_secrets(client.inner()).await?;
                self.secrets = raw
                    .into_iter()
                    .map(|s| {
                        let labels = s
                            .spec
                            .as_ref()
                            .and_then(|spec| spec.labels.as_ref())
                            .map(|l| {
                                l.iter()
                                    .map(|(k, v)| format!("{}={}", k, v))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                            .unwrap_or_default();
                        crate::models::secret::SecretRow {
                            id: s.id.unwrap_or_default(),
                            name: s.spec.unwrap_or_default().name.unwrap_or_default(),
                            created_at: s.created_at.unwrap_or_default(),
                            labels,
                        }
                    })
                    .collect();
                self.total_items = self.secrets.len();
            }
            AppState::Tasks => {
                let raw = crate::api::task::list_tasks(client.inner()).await?;
                self.tasks = raw
                    .into_iter()
                    .map(|t| {
                        let labels = t
                            .spec
                            .as_ref()
                            .and_then(|s| s.container_spec.as_ref())
                            .and_then(|c| c.labels.as_ref())
                            .map(|l| {
                                l.iter()
                                    .map(|(k, v)| format!("{}={}", k, v))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                            .unwrap_or_default();
                        crate::models::task::TaskRow {
                            id: t.id.unwrap_or_default(),
                            name: t.name.unwrap_or_default(),
                            desired_state: format!(
                                "{:?}",
                                t.desired_state
                                    .unwrap_or(bollard::models::TaskState::RUNNING)
                            ),
                            current_state: t
                                .status
                                .as_ref()
                                .and_then(|s| s.state)
                                .map(|v| format!("{:?}", v))
                                .unwrap_or_default(),
                            image: t
                                .spec
                                .as_ref()
                                .and_then(|s| s.container_spec.as_ref())
                                .and_then(|c| c.image.clone())
                                .unwrap_or_default(),
                            ports: "".to_string(),
                            node: t.node_id.unwrap_or_default(),
                            labels,
                        }
                    })
                    .collect();
                self.total_items = self.tasks.len();
            }
            AppState::Events => {
                self.fetch_events(client).await?;
                self.total_items = self.events.len();
            }
            AppState::Logs => {
                self.fetch_logs_services(client).await?;
                self.total_items = self.logs_services.len();
            }
        }
        Ok(())
    }

    async fn fetch_events(&mut self, client: &DockerClient) -> anyhow::Result<()> {
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
        let mut count = 0;
        while let Some(result) = stream.next().await {
            if count >= 200 {
                break;
            }
            if let Ok(ev) = result {
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
                count += 1;
            }
        }
        self.events = events;
        Ok(())
    }

    async fn fetch_logs_services(&mut self, client: &DockerClient) -> anyhow::Result<()> {
        let services = crate::api::service::list_services(client.inner()).await?;
        self.logs_services = services
            .iter()
            .filter_map(|s| {
                let name = s.spec.as_ref().and_then(|sp| sp.name.clone());
                let id = s.id.clone().unwrap_or_default();
                name.map(|n| (n, id))
            })
            .collect();
        self.selected_log_service = 0;
        self.fetch_selected_logs(client).await
    }

    async fn fetch_selected_logs(&mut self, client: &DockerClient) -> anyhow::Result<()> {
        if self.logs_services.is_empty() {
            self.current_logs = String::new();
            return Ok(());
        }
        let name = &self.logs_services[self.selected_log_service].0;
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
                Ok(line) => {
                    let s = line.to_string();
                    logs.push_str(&s);
                }
                Err(e) => {
                    logs.push_str(&format!("[Error: {}]\n", e));
                    break;
                }
            }
        }
        self.current_logs = logs;
        Ok(())
    }
}

pub async fn run_tui(client: &DockerClient) -> anyhow::Result<()> {
    let host_info = match client.inner().info().await {
        Ok(info) => {
            let name = info.name.unwrap_or_else(|| "docker".to_string());
            format!("docker@{}", name)
        }
        Err(_) => "docker".to_string(),
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(host_info);
    app.refresh(client).await?;

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
    client: &DockerClient,
) -> anyhow::Result<()> {
    let poll_interval = Duration::from_millis(100);

    loop {
        let now = Instant::now();

        if app.auto_refresh && now.duration_since(app.last_refresh) >= Duration::from_secs(app.auto_refresh_interval) {
            app.refresh(client).await?;
        }

        terminal.draw(|f| ui(f, app))?;

        if event::poll(poll_interval)? {
            if let Event::Key(key) = event::read()?
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
                            let total = app.filtered_item_count();
                            if total > 0 {
                                app.selected_index = (app.selected_index + 1).min(total - 1);
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            app.selected_index = app.selected_index.saturating_sub(1);
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
}

fn ui(f: &mut ratatui::Frame, app: &App) {
    let size = f.area();

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Length(1), // tabs
            Constraint::Min(0),   // content
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
        Span::styled(auto_status, Style::default().fg(if app.auto_refresh { COLOR_SUCCESS } else { COLOR_DIM })),
        Span::styled(
            "  │  ",
            Style::default().fg(COLOR_DIM),
        ),
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

    let header = Paragraph::new(header_text).style(
        Style::default()
            .bg(COLOR_HEADER_BG)
            .fg(Color::White),
    );
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
    Row::new(cols.iter().map(|c| Cell::from(*c).style(
        Style::default()
            .fg(COLOR_PRIMARY)
            .add_modifier(Modifier::BOLD),
    )))
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
    let filtered: Vec<&crate::models::service::ServiceRow> = if app.search_query.is_empty() {
        app.services.iter().collect()
    } else {
        app.services
            .iter()
            .filter(|s| s.name.to_lowercase().contains(&app.search_query.to_lowercase()))
            .collect()
    };

    let header = make_header_row(&["", "Name", "Mode", "Replicas", "Image"]);

    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_selected = i == app.selected_index;
            let (icon, color) = App::status_icon(&s.replicas);
            let style = selected_style(is_selected);

            Row::new(vec![
                Cell::from(Span::styled(icon, Style::default().fg(color))),
                Cell::from(Span::styled(&s.name, style)),
                Cell::from(Span::styled(&s.mode, style)),
                Cell::from(Span::styled(&s.replicas, style)),
                Cell::from(Span::styled(
                    truncate(&s.image, 40),
                    style.fg(COLOR_DIM),
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
            Constraint::Min(20),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_DIM))
            .title(format!(
                " Services ({}) ",
                app.filtered_item_count()
            ))
            .title_style(Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
    )
    .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_widget(table, area);
}

fn render_nodes_table(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered: Vec<&crate::models::node::NodeRow> = if app.search_query.is_empty() {
        app.nodes.iter().collect()
    } else {
        app.nodes
            .iter()
            .filter(|n| n.hostname.to_lowercase().contains(&app.search_query.to_lowercase()))
            .collect()
    };

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
                Cell::from(Span::styled(
                    &n.availability,
                    style.fg(avail_color),
                )),
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
            .title(format!(
                " Nodes ({}) ",
                app.filtered_item_count()
            ))
            .title_style(Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
    );

    f.render_widget(table, area);
}

fn render_networks_table(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered: Vec<&crate::models::network::NetworkRow> = if app.search_query.is_empty() {
        app.networks.iter().collect()
    } else {
        app.networks
            .iter()
            .filter(|n| n.name.to_lowercase().contains(&app.search_query.to_lowercase()))
            .collect()
    };

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
            .title(format!(
                " Networks ({}) ",
                app.filtered_item_count()
            ))
            .title_style(Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
    );

    f.render_widget(table, area);
}

fn render_ports_table(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered: Vec<&crate::models::port::ServicePortInfo> = if app.search_query.is_empty() {
        app.ports.iter().collect()
    } else {
        app.ports
            .iter()
            .filter(|p| p.service_name.to_lowercase().contains(&app.search_query.to_lowercase()))
            .collect()
    };

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
                    style.fg(if p.protocol == "tcp" { COLOR_SUCCESS } else { COLOR_WARNING }),
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
            .title(format!(
                " Ports ({}) ",
                app.filtered_item_count()
            ))
            .title_style(Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
    );

    f.render_widget(table, area);
}

fn render_secrets_table(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered: Vec<&crate::models::secret::SecretRow> = if app.search_query.is_empty() {
        app.secrets.iter().collect()
    } else {
        app.secrets
            .iter()
            .filter(|s| s.name.to_lowercase().contains(&app.search_query.to_lowercase()))
            .collect()
    };

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
            .title(format!(
                " Secrets ({}) ",
                app.filtered_item_count()
            ))
            .title_style(Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
    );

    f.render_widget(table, area);
}

fn render_tasks_table(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered: Vec<&crate::models::task::TaskRow> = if app.search_query.is_empty() {
        app.tasks.iter().collect()
    } else {
        app.tasks
            .iter()
            .filter(|t| t.name.to_lowercase().contains(&app.search_query.to_lowercase()))
            .collect()
    };

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
                Cell::from(Span::styled(
                    truncate(&t.node, 16),
                    style.fg(COLOR_DIM),
                )),
                Cell::from(Span::styled(
                    truncate(&t.image, 30),
                    style.fg(COLOR_DIM),
                )),
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
            .title(format!(
                " Tasks ({}) ",
                app.filtered_item_count()
            ))
            .title_style(Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
    );

    f.render_widget(table, area);
}

fn render_events_list(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let filtered: Vec<&EventEntry> = if app.search_query.is_empty() {
        app.events.iter().collect()
    } else {
        app.events
            .iter()
            .filter(|e| {
                e.action.to_lowercase().contains(&app.search_query.to_lowercase())
                    || e.id.to_lowercase().contains(&app.search_query.to_lowercase())
            })
            .collect()
    };

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
            .title(format!(
                " Events ({}) ",
                app.filtered_item_count()
            ))
            .title_style(Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
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
            .title_style(Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
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
            .title_style(Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
    );

    f.render_widget(log_list, chunks[1]);
}

fn render_detail_pane(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    match app.state {
        AppState::Services => {
            if let Some(s) = app.services.get(app.selected_index) {
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
            if let Some(n) = app.nodes.get(app.selected_index) {
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
            if let Some(t) = app.tasks.get(app.selected_index) {
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
            if let Some(n) = app.networks.get(app.selected_index) {
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
            if let Some(s) = app.secrets.get(app.selected_index) {
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
            if let Some(p) = app.ports.get(app.selected_index) {
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
            .title_style(Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
    );

    f.render_widget(detail, area);
}

fn render_footer(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let footer_text = if app.search_mode {
        Line::from(vec![
            Span::styled(" /", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{}█", app.search_query),
                Style::default().fg(Color::White),
            ),
            Span::styled("  Esc to cancel  Enter to confirm", Style::default().fg(COLOR_DIM)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" /", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::styled("Search  ", Style::default().fg(COLOR_DIM)),
            Span::styled("a", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::styled(
                if app.auto_refresh { "On " } else { "Off" },
                Style::default().fg(if app.auto_refresh { COLOR_SUCCESS } else { COLOR_DIM }),
            ),
            Span::styled("  ", Style::default()),
            Span::styled("+/-", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}s  ", app.auto_refresh_interval), Style::default().fg(COLOR_DIM)),
            Span::styled("d", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::styled("Detail  ", Style::default().fg(COLOR_DIM)),
            Span::styled("j/k", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::styled("Nav  ", Style::default().fg(COLOR_DIM)),
            Span::styled("Tab", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::styled("Switch  ", Style::default().fg(COLOR_DIM)),
            Span::styled("r", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::styled("Refresh  ", Style::default().fg(COLOR_DIM)),
            Span::styled("q", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::styled("Quit", Style::default().fg(COLOR_DIM)),
        ])
    };

    let footer = Paragraph::new(footer_text).style(
        Style::default()
            .bg(COLOR_HEADER_BG)
            .fg(Color::White),
    );
    f.render_widget(footer, area);
}

fn detail_line(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:12}", label),
            Style::default().fg(COLOR_DIM),
        ),
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
