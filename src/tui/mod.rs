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
    widgets::*,
};
use std::io;

pub enum AppState {
    Services,
    Nodes,
    Ports,
    Networks,
    Secrets,
    Tasks,
    Events,
    Logs,
}

pub struct App {
    pub state: AppState,
    pub services: Vec<crate::models::service::ServiceRow>,
    pub nodes: Vec<crate::models::node::NodeRow>,
    pub networks: Vec<crate::models::network::NetworkRow>,
    pub ports: Vec<crate::api::port::PortSummary>,
    pub secrets: Vec<crate::models::secret::SecretRow>,
    pub tasks: Vec<crate::models::task::TaskRow>,
    pub selected_index: usize,
    pub events: Vec<String>,
    pub logs_services: Vec<(String, String)>,
    pub selected_log_service: usize,
    pub current_logs: String,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: AppState::Services,
            services: Vec::new(),
            nodes: Vec::new(),
            networks: Vec::new(),
            ports: Vec::new(),
            secrets: Vec::new(),
            tasks: Vec::new(),
            selected_index: 0,
            events: Vec::new(),
            logs_services: Vec::new(),
            selected_log_service: 0,
            current_logs: String::new(),
        }
    }

    pub async fn refresh(&mut self, client: &DockerClient) -> anyhow::Result<()> {
        match self.state {
            AppState::Services => {
                let raw_services = crate::api::service::list_services(client.inner()).await?;
                self.services = raw_services
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
                                    .join(",")
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
            }
            AppState::Nodes => {
                let raw_nodes = crate::api::node::list_nodes(client.inner()).await?;
                self.nodes = raw_nodes
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
                                    .join(",")
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
            }
            AppState::Networks => {
                let raw_networks = crate::api::network::list_networks(client.inner()).await?;
                self.networks = raw_networks
                    .into_iter()
                    .map(|n| {
                        let labels = n
                            .labels
                            .as_ref()
                            .map(|l| {
                                l.iter()
                                    .map(|(k, v)| format!("{}={}", k, v))
                                    .collect::<Vec<_>>()
                                    .join(",")
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
            }
            AppState::Ports => {
                let summary = crate::api::port::get_port_summary(client.inner()).await?;
                self.ports = vec![summary];
            }
            AppState::Secrets => {
                let raw_secrets = crate::api::secret::list_secrets(client.inner()).await?;
                self.secrets = raw_secrets
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
                                    .join(",")
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
            }
            AppState::Tasks => {
                let raw_tasks = crate::api::task::list_tasks(client.inner()).await?;
                self.tasks = raw_tasks
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
                                    .join(",")
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
            }
            AppState::Events => {
                self.fetch_events(client).await?;
            }
            AppState::Logs => {
                self.fetch_logs_services(client).await?;
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
                let type_ = ev.typ.map(|t| format!("{:?}", t)).unwrap_or_default();
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
                events.push(format!("[{}] {} {}: {}", time_str, type_, action, id));
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
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
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
    loop {
        terminal.draw(|f| ui(f, app))?;

        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('r') => {
                    app.refresh(client).await?;
                }
                KeyCode::Char('j') | KeyCode::Down => match app.state {
                    AppState::Logs => {
                        if !app.logs_services.is_empty() {
                            app.selected_log_service = app
                                .selected_log_service
                                .saturating_add(1)
                                .min(app.logs_services.len().saturating_sub(1));
                        }
                    }
                    _ => {
                        app.selected_index = app.selected_index.saturating_add(1);
                    }
                },
                KeyCode::Char('k') | KeyCode::Up => match app.state {
                    AppState::Logs => {
                        app.selected_log_service = app.selected_log_service.saturating_sub(1);
                    }
                    _ => {
                        app.selected_index = app.selected_index.saturating_sub(1);
                    }
                },
                KeyCode::Enter => {
                    if matches!(app.state, AppState::Logs) {
                        app.fetch_selected_logs(client).await?;
                    }
                }
                KeyCode::Tab => {
                    app.state = match app.state {
                        AppState::Services => AppState::Nodes,
                        AppState::Nodes => AppState::Networks,
                        AppState::Networks => AppState::Ports,
                        AppState::Ports => AppState::Secrets,
                        AppState::Secrets => AppState::Tasks,
                        AppState::Tasks => AppState::Events,
                        AppState::Events => AppState::Logs,
                        AppState::Logs => AppState::Services,
                    };
                    app.selected_index = 0;
                    app.refresh(client).await?;
                }
                KeyCode::Char('1') => {
                    app.state = AppState::Services;
                    app.refresh(client).await?;
                }
                KeyCode::Char('2') => {
                    app.state = AppState::Nodes;
                    app.refresh(client).await?;
                }
                KeyCode::Char('3') => {
                    app.state = AppState::Networks;
                    app.refresh(client).await?;
                }
                KeyCode::Char('4') => {
                    app.state = AppState::Ports;
                    app.refresh(client).await?;
                }
                KeyCode::Char('5') => {
                    app.state = AppState::Secrets;
                    app.refresh(client).await?;
                }
                KeyCode::Char('6') => {
                    app.state = AppState::Tasks;
                    app.refresh(client).await?;
                }
                KeyCode::Char('7') => {
                    app.state = AppState::Events;
                    app.refresh(client).await?;
                }
                KeyCode::Char('8') => {
                    app.state = AppState::Logs;
                    app.refresh(client).await?;
                }
                _ => {}
            }
        }
    }
}

fn ui(f: &mut ratatui::Frame, app: &App) {
    let size = f.area();

    let title = match app.state {
        AppState::Services => "Services",
        AppState::Nodes => "Nodes",
        AppState::Networks => "Networks",
        AppState::Ports => "Ports",
        AppState::Secrets => "Secrets",
        AppState::Tasks => "Tasks",
        AppState::Events => "Events",
        AppState::Logs => "Logs",
    };

    let tabs = Paragraph::new(
        format!(
            " [1]Services [2]Nodes [3]Networks [4]Ports [5]Secrets [6]Tasks [7]Events [8]Logs | Current: {} | [r]Refresh [q]Quit ",
            title
        )
    )
    .style(ratatui::style::Style::default().fg(ratatui::style::Color::White));

    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(size);

    f.render_widget(
        tabs.block(
            Block::default()
                .borders(Borders::ALL)
                .title(" SwarmCtl Dashboard "),
        ),
        chunks[0],
    );

    match app.state {
        AppState::Services => f.render_widget(render_services(app), chunks[1]),
        AppState::Nodes => f.render_widget(render_nodes(app), chunks[1]),
        AppState::Networks => f.render_widget(render_networks(app), chunks[1]),
        AppState::Ports => f.render_widget(render_ports(app), chunks[1]),
        AppState::Secrets => f.render_widget(render_secrets(app), chunks[1]),
        AppState::Tasks => f.render_widget(render_tasks(app), chunks[1]),
        AppState::Events => f.render_widget(render_events(app), chunks[1]),
        AppState::Logs => render_logs(f, app, chunks[1]),
    }

    let help = Paragraph::new(
        " [j/k] Navigate | [Tab] Switch View | [r] Refresh | [Enter] Load Logs | [q] Quit ",
    );
    f.render_widget(
        help.block(Block::default().borders(Borders::ALL)),
        chunks[2],
    );
}

fn render_services(app: &App) -> List<'_> {
    let items: Vec<ListItem> = app
        .services
        .iter()
        .map(|s| {
            ListItem::new(format!(
                "{:12} {:30} {:12} {:10} {}",
                &s.id[..12.min(s.id.len())],
                s.name,
                s.mode,
                s.replicas,
                s.image.chars().take(40).collect::<String>()
            ))
        })
        .collect();

    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Services ({})", app.services.len())),
    )
}

fn render_nodes(app: &App) -> List<'_> {
    let items: Vec<ListItem> = app
        .nodes
        .iter()
        .map(|n| {
            ListItem::new(format!(
                "{:12} {:20} {:15} {:10} {}",
                &n.id[..12.min(n.id.len())],
                n.hostname,
                n.status,
                n.availability,
                n.manager
            ))
        })
        .collect();

    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Nodes ({})", app.nodes.len())),
    )
}

fn render_networks(app: &App) -> List<'_> {
    let items: Vec<ListItem> = app
        .networks
        .iter()
        .map(|n| {
            ListItem::new(format!(
                "{:12} {:30} {:12} {:10} {}",
                &n.id[..12.min(n.id.len())],
                n.name,
                n.driver,
                n.scope,
                n.internal
            ))
        })
        .collect();

    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Networks ({})", app.networks.len())),
    )
}

fn render_ports(app: &App) -> List<'_> {
    let summary = app.ports.first();

    let items: Vec<ListItem> = if let Some(s) = summary {
        let mut all_items = Vec::new();

        all_items.push(ListItem::new(format!(
            "TCP Ports: {}",
            s.used_tcp
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
        all_items.push(ListItem::new(format!(
            "UDP Ports: {}",
            s.used_udp
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
        all_items.push(ListItem::new("Port Mappings:".to_string()));
        for m in &s.port_mappings {
            all_items.push(ListItem::new(format!(
                "  {}:{} -> {} ({}:{})",
                m.0, m.1, m.2, m.3, m.4
            )));
        }
        all_items
    } else {
        vec![ListItem::new("No port data available".to_string())]
    };

    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Port Mappings"),
    )
}

fn render_secrets(app: &App) -> List<'_> {
    let items: Vec<ListItem> = app
        .secrets
        .iter()
        .map(|s| ListItem::new(format!("{:12} {}", &s.id[..12.min(s.id.len())], s.name)))
        .collect();

    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Secrets ({})", app.secrets.len())),
    )
}

fn render_tasks(app: &App) -> List<'_> {
    let items: Vec<ListItem> = app
        .tasks
        .iter()
        .map(|t| {
            ListItem::new(format!(
                "{:12} {:30} {:15} {:12}",
                &t.id[..12.min(t.id.len())],
                t.name,
                t.current_state,
                t.node
            ))
        })
        .collect();

    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Tasks ({})", app.tasks.len())),
    )
}

fn render_events(app: &App) -> List<'_> {
    let items: Vec<ListItem> = if app.events.is_empty() {
        vec![ListItem::new("No recent events".to_string())]
    } else {
        app.events
            .iter()
            .map(|e| ListItem::new(e.clone()))
            .collect()
    };

    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Events ({})", app.events.len())),
    )
}

fn render_logs(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(area);

    let service_items: Vec<ListItem> = if app.logs_services.is_empty() {
        vec![ListItem::new("No services found".to_string())]
    } else {
        app.logs_services
            .iter()
            .enumerate()
            .map(|(i, (name, _))| {
                let prefix = if i == app.selected_log_service {
                    " > "
                } else {
                    "   "
                };
                ListItem::new(format!("{}{}", prefix, name))
            })
            .collect()
    };

    let services_list = List::new(service_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Services (j/k, Enter to load logs)"),
    );

    f.render_widget(services_list, chunks[0]);

    let log_lines: Vec<ListItem> = if app.current_logs.is_empty() {
        vec![ListItem::new(
            "Select a service and press Enter to view logs".to_string(),
        )]
    } else {
        app.current_logs
            .lines()
            .map(|line| ListItem::new(line.to_string()))
            .collect()
    };

    let log_list = List::new(log_lines).block(
        Block::default().borders(Borders::ALL).title(format!(
            "Logs: {}",
            app.logs_services
                .get(app.selected_log_service)
                .map(|(n, _)| n.as_str())
                .unwrap_or("-")
        )),
    );

    f.render_widget(log_list, chunks[1]);
}
