use crate::api::client::DockerClient;
use ratatui::{
    backend::CrosstermBackend,
    widgets::*,
    Terminal,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use anyhow::Result;
use std::io;

pub enum AppState {
    Services,
    Nodes,
    Ports,
    Networks,
    Secrets,
    Tasks,
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
        }
    }

    pub async fn refresh(&mut self, client: &DockerClient) -> anyhow::Result<()> {
        match self.state {
            AppState::Services => {
                let raw_services = crate::api::service::list_services(client.inner()).await?;
                self.services = raw_services.into_iter().map(|s| {
                    let spec = s.spec.unwrap_or_default();
                    let name = spec.name.unwrap_or_default();
                    let image = spec.task_template.and_then(|t| t.container_spec).and_then(|c| c.image).unwrap_or_default();
                    let (mode, replicas) = match spec.mode {
                        Some(m) if m.replicated.is_some() => {
                            let r = m.replicated.unwrap().replicas.unwrap_or(0);
                            ("replicated".to_string(), format!("{}/{}", r, r))
                        }
                        Some(_) => ("global".to_string(), "N/A".to_string()),
                        None => ("unknown".to_string(), "N/A".to_string()),
                    };
                    crate::models::service::ServiceRow { id: s.id.unwrap_or_default(), name, mode, replicas, image }
                }).collect();
            }
            AppState::Nodes => {
                let raw_nodes = crate::api::node::list_nodes(client.inner()).await?;
                self.nodes = raw_nodes.into_iter().map(|n| {
                    let spec = n.spec.unwrap_or_default();
                    let status = n.status.unwrap_or_default();
                    let manager = n.manager_status.as_ref().map(|m| {
                        match m.reachability.unwrap_or(bollard::models::Reachability::UNKNOWN) {
                            bollard::models::Reachability::REACHABLE => "Reachable",
                            bollard::models::Reachability::UNREACHABLE => "Unavailable",
                            _ => "-",
                        }
                    }).unwrap_or("-");
                    crate::models::node::NodeRow {
                        id: n.id.unwrap_or_default(),
                        hostname: spec.name.unwrap_or_default(),
                        status: status.state.unwrap_or(bollard::models::NodeState::READY).to_string(),
                        availability: spec.availability.unwrap_or(bollard::models::NodeSpecAvailabilityEnum::ACTIVE).to_string(),
                        manager: manager.to_string(),
                    }
                }).collect();
            }
            AppState::Networks => {
                let raw_networks = crate::api::network::list_networks(client.inner()).await?;
                self.networks = raw_networks.into_iter().map(|n| {
                    crate::models::network::NetworkRow {
                        id: n.id.unwrap_or_default(),
                        name: n.name.unwrap_or_default(),
                        driver: n.driver.unwrap_or_else(|| "unknown".to_string()),
                        scope: n.scope.unwrap_or_else(|| "unknown".to_string()),
                        internal: if n.internal.unwrap_or(false) { "true" } else { "false" }.to_string(),
                    }
                }).collect();
            }
            AppState::Ports => {
                let summary = crate::api::port::get_port_summary(client.inner()).await?;
                self.ports = vec![summary];
            }
            AppState::Secrets => {
                let raw_secrets = crate::api::secret::list_secrets(client.inner()).await?;
                self.secrets = raw_secrets.into_iter().map(|s| {
                    crate::models::secret::SecretRow {
                        id: s.id.unwrap_or_default(),
                        name: s.spec.unwrap_or_default().name.unwrap_or_default(),
                        created_at: s.created_at.unwrap_or_default(),
                    }
                }).collect();
            }
            AppState::Tasks => {
                let raw_tasks = crate::api::task::list_tasks(client.inner()).await?;
                self.tasks = raw_tasks.into_iter().map(|t| {
                    crate::models::task::TaskRow {
                        id: t.id.unwrap_or_default(),
                        name: t.name.unwrap_or_default(),
                        desired_state: format!("{:?}", t.desired_state.unwrap_or(bollard::models::TaskState::RUNNING)),
                        current_state: t.status.as_ref().and_then(|s| s.state.clone()).map(|v| format!("{:?}", v)).unwrap_or_default(),
                        image: t.spec.as_ref().and_then(|s| s.container_spec.as_ref()).and_then(|c| c.image.clone()).unwrap_or_default(),
                        ports: "".to_string(),
                        node: t.node_id.unwrap_or_default(),
                    }
                }).collect();
            }
        }
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

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('r') => { app.refresh(client).await?; }
                    KeyCode::Char('j') | KeyCode::Down => {
                        app.selected_index = app.selected_index.saturating_add(1);
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        app.selected_index = app.selected_index.saturating_sub(1);
                    }
                    KeyCode::Tab => {
                        app.state = match app.state {
                            AppState::Services => AppState::Nodes,
                            AppState::Nodes => AppState::Networks,
                            AppState::Networks => AppState::Ports,
                            AppState::Ports => AppState::Secrets,
                            AppState::Secrets => AppState::Tasks,
                            AppState::Tasks => AppState::Services,
                        };
                        app.selected_index = 0;
                        app.refresh(client).await?;
                    }
                    KeyCode::Char('1') => { app.state = AppState::Services; app.refresh(client).await?; }
                    KeyCode::Char('2') => { app.state = AppState::Nodes; app.refresh(client).await?; }
                    KeyCode::Char('3') => { app.state = AppState::Networks; app.refresh(client).await?; }
                    KeyCode::Char('4') => { app.state = AppState::Ports; app.refresh(client).await?; }
                    KeyCode::Char('5') => { app.state = AppState::Secrets; app.refresh(client).await?; }
                    KeyCode::Char('6') => { app.state = AppState::Tasks; app.refresh(client).await?; }
                    _ => {}
                }
            }
        }
    }
}

fn ui(f: &mut ratatui::Frame, app: &App) {
    let size = f.size();
    
    let title = match app.state {
        AppState::Services => "Services",
        AppState::Nodes => "Nodes",
        AppState::Networks => "Networks",
        AppState::Ports => "Ports",
        AppState::Secrets => "Secrets",
        AppState::Tasks => "Tasks",
    };

    let tabs = Paragraph::new(
        format!(
            " [1]Services [2]Nodes [3]Networks [4]Ports [5]Secrets [6]Tasks | Current: {} | [r]Refresh [q]Quit ",
            title
        )
    )
    .style(ratatui::style::Style::default().fg(ratatui::style::Color::White));

    let items = match app.state {
        AppState::Services => render_services(app),
        AppState::Nodes => render_nodes(app),
        AppState::Networks => render_networks(app),
        AppState::Ports => render_ports(app),
        AppState::Secrets => render_secrets(app),
        AppState::Tasks => render_tasks(app),
    };

    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(3),
            ratatui::layout::Constraint::Min(0),
            ratatui::layout::Constraint::Length(3),
        ])
        .split(size);

    f.render_widget(tabs.block(
        Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .title(" SwarmCtl Dashboard ")
    ), chunks[0]);

    f.render_widget(items, chunks[1]);

    let help = Paragraph::new(" [j/k] Navigate | [Tab] Switch View | [r] Refresh | [q] Quit ");
    f.render_widget(help.block(
        Block::default().borders(ratatui::widgets::Borders::ALL)
    ), chunks[2]);
}

fn render_services(app: &App) -> List {
    let items: Vec<ListItem> = app.services.iter().map(|s| {
        ListItem::new(format!(
            "{:12} {:30} {:12} {:10} {}",
            &s.id[..12.min(s.id.len())],
            s.name,
            s.mode,
            s.replicas,
            s.image.chars().take(40).collect::<String>()
        ))
    }).collect();

    List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!("Services ({})", app.services.len())))
}

fn render_nodes(app: &App) -> List {
    let items: Vec<ListItem> = app.nodes.iter().map(|n| {
        ListItem::new(format!(
            "{:12} {:20} {:15} {:10} {}",
            &n.id[..12.min(n.id.len())],
            n.hostname,
            n.status,
            n.availability,
            n.manager
        ))
    }).collect();

    List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!("Nodes ({})", app.nodes.len())))
}

fn render_networks(app: &App) -> List {
    let items: Vec<ListItem> = app.networks.iter().map(|n| {
        ListItem::new(format!(
            "{:12} {:30} {:12} {:10} {}",
            &n.id[..12.min(n.id.len())],
            n.name,
            n.driver,
            n.scope,
            n.internal
        ))
    }).collect();

    List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!("Networks ({})", app.networks.len())))
}

fn render_ports(app: &App) -> List {
    let summary = app.ports.first();
    
    let items: Vec<ListItem> = if let Some(s) = summary {
        let mut all_items = Vec::new();
        
        all_items.push(ListItem::new(format!("TCP Ports: {}", s.used_tcp.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", "))));
        all_items.push(ListItem::new(format!("UDP Ports: {}", s.used_udp.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", "))));
        all_items.push(ListItem::new("Port Mappings:".to_string()));
        for m in &s.port_mappings {
            all_items.push(ListItem::new(format!("  {}:{} -> {} ({}:{})", m.0, m.1, m.2, m.3, m.4)));
        }
        all_items
    } else {
        vec![ListItem::new("No port data available".to_string())]
    };

    List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Port Mappings"))
}

fn render_secrets(app: &App) -> List {
    let items: Vec<ListItem> = app.secrets.iter().map(|s| {
        ListItem::new(format!(
            "{:12} {}",
            &s.id[..12.min(s.id.len())],
            s.name
        ))
    }).collect();

    List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!("Secrets ({})", app.secrets.len())))
}

fn render_tasks(app: &App) -> List {
    let items: Vec<ListItem> = app.tasks.iter().map(|t| {
        ListItem::new(format!(
            "{:12} {:30} {:15} {:12}",
            &t.id[..12.min(t.id.len())],
            t.name,
            t.current_state,
            t.node
        ))
    }).collect();

    List::new(items)
        .block(Block::default().borders(Borders::ALL).title(format!("Tasks ({})", app.tasks.len())))
}
