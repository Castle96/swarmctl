use crate::api::client::DockerClient;
use crate::cli::root::OutputFormat;
use crate::models::port::{PortRow, ServicePortInfo};
use crate::utils::printer::{print_table, print_json, print_yaml};
use crate::api::port::{list_used_ports, list_service_ports, get_available_ports, get_port_summary};
use anyhow::Result;

pub async fn run(
    client: &DockerClient, 
    output: OutputFormat,
    show_available: bool,
    range_start: Option<u16>,
    range_end: Option<u16>,
    protocol: Option<String>,
) -> Result<()> {
    match output {
        OutputFormat::Table => {
            let ports = list_service_ports(client.inner()).await?;
            
            let filtered: Vec<ServicePortInfo> = if let Some(ref proto) = protocol {
                ports.into_iter()
                    .filter(|p| p.protocol.to_lowercase() == proto.to_lowercase())
                    .collect()
            } else {
                ports
            };
            
            print_ports_table(&filtered);
            
            if show_available {
                let start = range_start.unwrap_or(30000);
                let end = range_end.unwrap_or(40000);
                let available = get_available_ports(client.inner(), start, end, protocol.as_deref()).await?;
                print_available_ports(&available, start, end);
            }
        }
        OutputFormat::Json => {
            let ports = list_used_ports(client.inner()).await?;
            print_json(&ports)?;
        }
        OutputFormat::Yaml => {
            let ports = list_used_ports(client.inner()).await?;
            print_yaml(&ports)?;
        }
    }
    Ok(())
}

fn print_ports_table(ports: &[ServicePortInfo]) {
    println!();
    println!("{}", style("════════════════════════════════════════════════════════════════════════════════", 80, '─'));
    println!("  \x1b[1;36mSwarm Port Mappings\x1b[0m");
    println!("{}", style("────────────────────────────────────────────────────────────────────────────────", 80, '─'));
    println!("  {:<30} {:>8} {:>8} {:>8} {:>10}", 
        "\x1b[1mSERVICE\x1b[0m",
        "\x1b[1mPUBLISHED\x1b[0m",
        "\x1b[1mTARGET\x1b[0m",
        "\x1b[1mPROTO\x1b[0m",
        "\x1b[1mMODE\x1b[0m"
    );
    println!("{}", style("────────────────────────────────────────────────────────────────────────────────", 80, '─'));
    
    for port in ports {
        let service_truncated = truncate(&port.service_name, 28);
        let published = style_port(&port.published_port, true);
        let target = style_port(&port.target_port, false);
        
        let proto_colored = match port.protocol.to_lowercase().as_str() {
            "tcp" => format!("\x1b[32m{}\x1b[0m", port.protocol.to_uppercase()),
            "udp" => format!("\x1b[33m{}\x1b[0m", port.protocol.to_uppercase()),
            _ => format!("\x1b[37m{}\x1b[0m", port.protocol.to_uppercase()),
        };
        
        let mode_colored = match port.publish_mode.to_lowercase().as_str() {
            "ingress" => format!("\x1b[36m{}\x1b[0m", port.publish_mode),
            "host" => format!("\x1b[35m{}\x1b[0m", port.publish_mode),
            _ => port.publish_mode.clone(),
        };
        
        println!("  {:<30} {:>8} {:>8} {:>8} {:>10}", 
            truncate(&service_truncated, 30),
            published,
            target,
            proto_colored,
            mode_colored
        );
    }
    println!("{}", style("────────────────────────────────────────────────────────────────────────────────", 80, '─'));
    let unique_services = ports.iter().map(|p| &p.service_name).collect::<std::collections::HashSet<_>>().len();
    println!("  \x1b[1mTotal:\x1b[0m \x1b[32m{}\x1b[0m ports mapped across \x1b[36m{}\x1b[0m services", 
        ports.len(), 
        unique_services
    );
    println!();
}

fn print_available_ports(available: &[u16], start: u16, end: u16) {
    println!();
    println!("{}", style("════════════════════════════════════════════════════════════════════════════════", 80, '─'));
    println!("  \x1b[1;33mAvailable Ports\x1b[0m (Range: {} - {})", start, end);
    println!("{}", style("────────────────────────────────────────────────────────────────────────────────", 80, '─'));
    
    if available.is_empty() {
        println!("  \x1b[31mNo available ports in the specified range.\x1b[0m");
    } else if available.len() <= 20 {
        for chunk in available.chunks(10) {
            println!("  {}", chunk.iter().map(|p| format!("\x1b[32m{}\x1b[0m", p)).collect::<Vec<_>>().join("  "));
        }
    } else {
        println!("  First 20 available ports:");
        for chunk in available.iter().take(20).collect::<Vec<_>>().chunks(10) {
            println!("  {}", chunk.iter().map(|p| format!("\x1b[32m{}\x1b[0m", p)).collect::<Vec<_>>().join("  "));
        }
        println!("  ... and {} more available ports", available.len() - 20);
    }
    
    println!("{}", style("────────────────────────────────────────────────────────────────────────────────", 80, '─'));
    println!("  \x1b[1mTotal Available:\x1b[0m \x1b[32m{}\x1b[0m ports", available.len());
    println!();
}

fn style(s: &str, width: usize, fill: char) -> String {
    let len = s.len();
    if len >= width {
        s[..width].to_string()
    } else {
        format!("{}{}", s, fill.to_string().repeat(width - len))
    }
}

fn style_port(port: &str, is_published: bool) -> String {
    if port == "?" || port == "0" || port.is_empty() {
        format!("\x1b[90m--\x1b[0m")
    } else {
        if is_published {
            format!("\x1b[32m{}\x1b[0m", port)
        } else {
            format!("\x1b[36m{}\x1b[0m", port)
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max-3])
    } else {
        s.to_string()
    }
}

pub async fn run_tui(client: &DockerClient) -> Result<()> {
    println!("\x1b[2J\x1b[H");
    println!("{}", style("╔════════════════════════════════════════════════════════════════════════════════════╗", 80, '═'));
    println!("{}", style("║                      SWARMCTL PORT VISUALIZER                                    ║", 80, '═'));
    println!("{}", style("╚════════════════════════════════════════════════════════════════════════════════════╝", 80, '═'));
    println!();
    println!("  Fetching port information from Docker Swarm...\n");
    
    let summary = get_port_summary(client.inner()).await?;
    
    println!("\x1b[2J\x1b[H");
    println!("{}", style("╔════════════════════════════════════════════════════════════════════════════════════╗", 80, '═'));
    println!("{}", style("║                      SWARMCTL PORT VISUALIZER                                    ║", 80, '═'));
    println!("{}", style("╚════════════════════════════════════════════════════════════════════════════════════╝", 80, '═'));
    println!();
    
    println!("  \x1b[1;37m┌─ Port Summary ─────────────────────────────────┐\x1b[0m");
    println!("  \x1b[1;37m│\x1b[0m");
    println!("  \x1b[1;37m│\x1b[0m  \x1b[32mTCP\x1b[0m Ports Used:   \x1b[1m{}\x1b[0m", summary.used_tcp.len());
    println!("  \x1b[1;37m│\x1b[0m  \x1b[33mUDP\x1b[0m Ports Used:   \x1b[1m{}\x1b[0m", summary.used_udp.len());
    println!("  \x1b[1;37m│\x1b[0m  \x1b[36mTotal Mappings:\x1b[0m \x1b[1m{}\x1b[0m", summary.port_mappings.len());
    println!("  \x1b[1;37m│\x1b[0m");
    println!("  \x1b[1;37m└──────────────────────────────────────────────┘\x1b[0m");
    println!();
    
    println!("  \x1b[1;37m┌─ TCP Port Range ──────────────────────────────┐\x1b[0m");
    if summary.used_tcp.is_empty() {
        println!("  \x1b[1;37m│\x1b[0m  \x1b[32mNo TCP ports in use\x1b[0m");
    } else {
        print!("  \x1b[1;37m│\x1b[0m  ");
        for (i, port) in summary.used_tcp.iter().enumerate() {
            if i > 0 && i % 8 == 0 {
                println!();
                print!("  \x1b[1;37m│\x1b[0m  ");
            }
            print!("\x1b[32m{}\x1b[0m ", port);
        }
        println!();
    }
    println!("  \x1b[1;37m└──────────────────────────────────────────────┘\x1b[0m");
    println!();
    
    if !summary.used_udp.is_empty() {
        println!("  \x1b[1;37m┌─ UDP Port Range ──────────────────────────────┐\x1b[0m");
        print!("  \x1b[1;37m│\x1b[0m  ");
        for (i, port) in summary.used_udp.iter().enumerate() {
            if i > 0 && i % 8 == 0 {
                println!();
                print!("  \x1b[1;37m│\x1b[0m  ");
            }
            print!("\x1b[33m{}\x1b[0m ", port);
        }
        println!();
        println!("  \x1b[1;37m└──────────────────────────────────────────────┘\x1b[0m");
        println!();
    }
    
    println!("  \x1b[1;37m┌─ Service Port Mappings ────────────────────────────────────────────────────┐\x1b[0m");
    println!("  \x1b[1;37m│\x1b[0m");
    for (i, mapping) in summary.port_mappings.iter().enumerate().take(10) {
        let (svc, pub_port, target, proto, mode) = mapping;
        let svc_trunc = truncate(svc, 20);
        let proto_str = match proto.as_str() {
            "tcp" => "\x1b[32mTCP\x1b[0m",
            "udp" => "\x1b[33mUDP\x1b[0m",
            _ => proto,
        };
        let mode_str = match mode.as_str() {
            "ingress" => "\x1b[36mingress\x1b[0m",
            "host" => "\x1b[35mhost\x1b[0m",
            _ => mode.as_str(),
        };
        println!("  \x1b[1;37m│\x1b[0m  {:>3}. {:<20} \x1b[32m{:>5}\x1b[0m -> \x1b[36m{}\x1b[0m ({}) [{}]", 
            i + 1, svc_trunc, pub_port, target, proto_str, mode_str);
    }
    if summary.port_mappings.len() > 10 {
        println!("  \x1b[1;37m│\x1b[0m  ... and {} more mappings", summary.port_mappings.len() - 10);
    }
    println!("  \x1b[1;37m│\x1b[0m");
    println!("  \x1b[1;37m└─────────────────────────────────────────────────────────────────────────────┘\x1b[0m");
    println!();
    println!("  \x1b[33mPress Ctrl+C to exit\x1b[0m");
    println!();
    
    Ok(())
}
