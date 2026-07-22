use crate::api::client::DockerClient;
use crate::api::discovery::{self, DiscoveredHost};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::*,
};
use std::io;

const COLOR_PRIMARY: Color = Color::Cyan;
const COLOR_SUCCESS: Color = Color::Green;
const COLOR_WARNING: Color = Color::Yellow;
const COLOR_DIM: Color = Color::DarkGray;
const COLOR_HEADER_BG: Color = Color::Rgb(40, 44, 52);
const COLOR_SELECTION_BG: Color = Color::Rgb(50, 54, 62);

enum TuiChoice {
    CreateNew,
    JoinHost(usize),
    Cancelled,
}

pub async fn run_discovery_tui(
    client: &DockerClient,
    subnet: Option<String>,
) -> Result<(), anyhow::Error> {
    println!("Scanning network for Docker hosts...\n");

    let hosts = discovery::scan_subnet(subnet.as_deref()).await?;

    if hosts.is_empty() {
        println!("No Docker hosts found on the network.");
        println!();
        println!("Would you like to initialize a new swarm on this node? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if input.trim().eq_ignore_ascii_case("y") {
            crate::cli::discover::run_interactive(client, None).await?;
        }
        return Ok(());
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = TuiState::new(hosts);
    let result = run_tui_app(&mut terminal, &mut state);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    let choice = result?;

    match choice {
        TuiChoice::Cancelled => {
            println!("Cancelled.");
        }
        TuiChoice::CreateNew => {
            crate::cli::discover::run_interactive(client, None).await?;
        }
        TuiChoice::JoinHost(idx) => {
            let mut selected = state.hosts[idx].clone();

            println!("Connecting to manager at {} to retrieve join tokens...", selected.ip);

            if selected.join_token_worker.is_none() {
                discovery::probe_host_for_tokens(&mut selected).await?;
            }

            let worker_token = selected.join_token_worker.as_deref()
                .ok_or_else(|| anyhow::anyhow!("Failed to retrieve join token from {}", selected.ip))?;
            let manager_token = selected.join_token_manager.as_deref();

            println!();
            println!("Join tokens retrieved:");
            println!("  Worker token:  {}...", &worker_token[..24.min(worker_token.len())]);
            if let Some(mt) = manager_token {
                println!("  Manager token: {}...", &mt[..24.min(mt.len())]);
            }

            println!();
            println!("Join as:");
            println!("  [1] Worker");
            if manager_token.is_some() {
                println!("  [2] Manager");
            }

            let role_choice = read_role_choice()?;
            let token = if role_choice == 2 && manager_token.is_some() {
                manager_token.unwrap()
            } else {
                worker_token
            };

            let advertise_addr = discovery::detect_local_ip()?.to_string();
            println!();
            println!("Joining swarm at {} (manager: {})...", selected.ip,
                selected.swarm_name.as_deref().unwrap_or("unnamed"));

            crate::api::swarm::join_swarm(
                client.inner(),
                &advertise_addr,
                &format!("{}:{}", selected.ip, discovery::SWARM_PORT),
                token,
            )
            .await?;

            println!("Successfully joined the swarm!");

            maybe_save_to_vault(client, worker_token, manager_token).await?;
        }
    }

    Ok(())
}

async fn maybe_save_to_vault(
    client: &DockerClient,
    worker_token: &str,
    manager_token: Option<&str>,
) -> Result<(), anyhow::Error> {
    if let Ok(vault_password) =
        rpassword::prompt_password("Vault password (to save tokens, or empty to skip): ")
    {
        if !vault_password.is_empty() {
            let vault = if crate::vault::LocalVault::exists() {
                crate::vault::LocalVault::open(&vault_password)?
            } else {
                crate::vault::LocalVault::create(&vault_password)?
            };
            let mut vault = vault;
            let host = std::env::var("DOCKER_HOST")
                .unwrap_or_else(|_| "unix:///var/run/docker.sock".to_string());
            let swarm_name = client
                .inner()
                .info()
                .await
                .ok()
                .and_then(|i| i.name)
                .unwrap_or_else(|| "docker".to_string());
            let tokens = crate::vault::models::JoinTokens {
                worker: worker_token.to_string(),
                manager: manager_token.unwrap_or_default().to_string(),
            };
            vault.store_swarm_tokens(tokens, None, &host, &swarm_name)?;
            println!("Tokens saved to vault.");
        }
    }
    Ok(())
}

fn read_role_choice() -> Result<usize, anyhow::Error> {
    use std::io::{self, BufRead, Write};
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    print!("> ");
    stdout.flush()?;
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    Ok(line.trim().parse::<usize>().unwrap_or(1))
}

struct TuiState {
    hosts: Vec<DiscoveredHost>,
    selected_index: usize,
    create_new_selected: bool,
}

impl TuiState {
    fn new(hosts: Vec<DiscoveredHost>) -> Self {
        Self {
            hosts,
            selected_index: 0,
            create_new_selected: false,
        }
    }

    fn move_up(&mut self) {
        if self.create_new_selected {
            self.create_new_selected = false;
            self.selected_index = self.hosts.len().saturating_sub(1);
        } else if self.selected_index > 0 {
            self.selected_index -= 1;
        } else {
            self.create_new_selected = true;
        }
    }

    fn move_down(&mut self) {
        if self.create_new_selected {
            self.create_new_selected = false;
            self.selected_index = 0;
        } else if self.selected_index < self.hosts.len().saturating_sub(1) {
            self.selected_index += 1;
        } else {
            self.create_new_selected = true;
        }
    }
}

fn run_tui_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut TuiState,
) -> Result<TuiChoice, anyhow::Error> {
    let poll_interval = std::time::Duration::from_millis(100);

    loop {
        terminal.draw(|f| ui(f, state))?;

        if event::poll(poll_interval)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            return Ok(TuiChoice::Cancelled);
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            state.move_down();
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            state.move_up();
                        }
                        KeyCode::Enter | KeyCode::Char('d') => {
                            if state.create_new_selected {
                                return Ok(TuiChoice::CreateNew);
                            } else {
                                return Ok(TuiChoice::JoinHost(state.selected_index));
                            }
                        }
                        KeyCode::Char('1') => {
                            if state.hosts.len() >= 1 {
                                state.create_new_selected = false;
                                state.selected_index = 0;
                            }
                        }
                        KeyCode::Char('2') => {
                            if state.hosts.len() >= 2 {
                                state.create_new_selected = false;
                                state.selected_index = 1;
                            }
                        }
                        KeyCode::Char('3') => {
                            if state.hosts.len() >= 3 {
                                state.create_new_selected = false;
                                state.selected_index = 2;
                            }
                        }
                        KeyCode::Char('4') => {
                            if state.hosts.len() >= 4 {
                                state.create_new_selected = false;
                                state.selected_index = 3;
                            }
                        }
                        KeyCode::Char('5') => {
                            if state.hosts.len() >= 5 {
                                state.create_new_selected = false;
                                state.selected_index = 4;
                            }
                        }
                        KeyCode::Char('6') => {
                            if state.hosts.len() >= 6 {
                                state.create_new_selected = false;
                                state.selected_index = 5;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn ui(f: &mut ratatui::Frame, state: &TuiState) {
    let size = f.area();

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(size);

    render_header(f, state, main_chunks[0]);
    render_hosts(f, state, main_chunks[1]);
    render_footer(f, state, main_chunks[2]);
}

fn render_header(f: &mut ratatui::Frame, _state: &TuiState, area: Rect) {
    let header = Paragraph::new(Line::from(vec![
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
        Span::styled(
            "Network Discovery",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  │  ", Style::default().fg(COLOR_DIM)),
        Span::styled(
            "Select a host to join, or create a new swarm",
            Style::default().fg(COLOR_DIM),
        ),
    ]))
    .style(
        Style::default()
            .bg(COLOR_HEADER_BG)
            .fg(Color::White),
    );
    f.render_widget(header, area);
}

fn render_hosts(f: &mut ratatui::Frame, state: &TuiState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    render_host_list(f, state, chunks[0]);
    render_host_detail(f, state, chunks[1]);
}

fn render_host_list(f: &mut ratatui::Frame, state: &TuiState, area: Rect) {
    let mut items: Vec<ListItem> = Vec::new();

    items.push(ListItem::new(Line::from(vec![
        Span::styled(
            "  ★ ",
            Style::default()
                .fg(COLOR_SUCCESS)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Create new swarm on this node",
            Style::default()
                .fg(COLOR_SUCCESS)
                .add_modifier(Modifier::BOLD),
        ),
    ])));

    for (i, host) in state.hosts.iter().enumerate() {
        let icon = if host.swarm_active { "●" } else { "○" };
        let icon_color = if host.swarm_active {
            COLOR_SUCCESS
        } else {
            COLOR_DIM
        };
        let swarm_marker = if host.swarm_active { " *" } else { "" };

        let host_line = Line::from(vec![
            Span::styled(format!("  {} ", icon), Style::default().fg(icon_color)),
            Span::styled(
                format!("[{}] {}", i + 1, host.ip),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                host.hostname
                    .as_deref()
                    .map(|h| format!(" ({})", h))
                    .unwrap_or_default(),
                Style::default().fg(COLOR_DIM),
            ),
            Span::styled(swarm_marker, Style::default().fg(COLOR_WARNING)),
            Span::styled(
                format!(
                    " {}",
                    if host.is_manager {
                        "[manager]"
                    } else if host.swarm_active {
                        "[worker]"
                    } else {
                        "[standalone]"
                    }
                ),
                Style::default().fg(if host.is_manager {
                    COLOR_PRIMARY
                } else if host.swarm_active {
                    COLOR_WARNING
                } else {
                    COLOR_DIM
                }),
            ),
        ]);

        items.push(ListItem::new(host_line));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_DIM))
                .title(format!(
                    " Discovered Hosts ({}) ",
                    state.hosts.len()
                ))
                .title_style(
                    Style::default()
                        .fg(COLOR_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .highlight_style(
            Style::default()
                .bg(COLOR_SELECTION_BG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    let mut list_state = ListState::default();
    let selected = if state.create_new_selected {
        0
    } else {
        state.selected_index + 1
    };
    list_state.select(Some(selected));

    f.render_stateful_widget(list, area, &mut list_state);
}

fn render_host_detail(f: &mut ratatui::Frame, state: &TuiState, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if state.create_new_selected {
        lines.push(Line::from(Span::styled(
            " Create New Swarm ",
            Style::default()
                .fg(COLOR_SUCCESS)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "This will initialize a new Docker Swarm",
            Style::default().fg(Color::White),
        )));
        lines.push(Line::from(Span::styled(
            "cluster on the current node.",
            Style::default().fg(Color::White),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "The node will become the first manager.",
            Style::default().fg(COLOR_DIM),
        )));
        lines.push(Line::from(Span::styled(
            "Join tokens will be displayed for other",
            Style::default().fg(COLOR_DIM),
        )));
        lines.push(Line::from(Span::styled(
            "nodes to connect.",
            Style::default().fg(COLOR_DIM),
        )));
    } else if let Some(host) = state.hosts.get(state.selected_index) {
        lines.push(Line::from(Span::styled(
            " Host Details ",
            Style::default()
                .fg(COLOR_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        lines.push(detail_line("IP Address", &host.ip));
        if let Some(ref h) = host.hostname {
            lines.push(detail_line("Hostname", h));
        }
        if let Some(ref v) = host.docker_version {
            lines.push(detail_line("Docker", v));
        }
        if let Some(dp) = host.docker_port {
            let dp_str = dp.to_string();
            lines.push(detail_line("Docker Port", &dp_str));
        }
        if let Some(sp) = host.swarm_port {
            let sp_str = sp.to_string();
            lines.push(detail_line("Swarm Port", &sp_str));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Swarm ",
            Style::default()
                .fg(COLOR_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(detail_line(
            "Active",
            if host.swarm_active { "yes" } else { "no" },
        ));
        if host.swarm_active {
            if let Some(ref name) = host.swarm_name {
                lines.push(detail_line("Name", name));
            }
            if let Some(ref id) = host.swarm_id {
                lines.push(detail_line("ID", &id[..16.min(id.len())]));
            }
            if let Some(n) = host.node_count {
                let n_str = n.to_string();
                lines.push(detail_line("Nodes", &n_str));
            }
            if let Some(m) = host.manager_count {
                let m_str = m.to_string();
                lines.push(detail_line("Managers", &m_str));
            }
            lines.push(detail_line(
                "This Node's Role",
                if host.is_manager {
                    "manager"
                } else {
                    "worker (not in this swarm)"
                },
            ));
        }
    }

    let paragraph = Paragraph::new(lines).block(
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

    f.render_widget(paragraph, area);
}

fn render_footer(f: &mut ratatui::Frame, _state: &TuiState, area: Rect) {
    let footer = Line::from(vec![
        Span::styled(" j/k", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
        Span::styled(" or ", Style::default().fg(COLOR_DIM)),
        Span::styled("↑/↓", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
        Span::styled(" navigate  ", Style::default().fg(COLOR_DIM)),
        Span::styled("Enter", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
        Span::styled(" select  ", Style::default().fg(COLOR_DIM)),
        Span::styled("q", Style::default().fg(COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
        Span::styled(" cancel", Style::default().fg(COLOR_DIM)),
    ]);

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(COLOR_DIM));

    let paragraph = Paragraph::new(footer).block(block);
    f.render_widget(paragraph, area);
}

fn detail_line(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:16}", key),
            Style::default()
                .fg(COLOR_DIM)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(value.to_string(), Style::default().fg(Color::White)),
    ])
}
