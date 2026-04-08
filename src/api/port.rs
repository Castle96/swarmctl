use bollard::Docker;
use bollard::query_parameters::ListServicesOptions;
use anyhow::Result;
use std::collections::HashSet;
use crate::models::port::{PortRow, ServicePortInfo};

pub async fn list_used_ports(docker: &Docker) -> Result<Vec<PortRow>> {
    let services = docker
        .list_services(None::<ListServicesOptions>)
        .await?;
    
    let mut used_ports = HashSet::new();
    
    for service in services {
        if let Some(spec) = service.spec {
            let service_name = spec.name.unwrap_or_default();
            if let Some(endpoint_spec) = spec.endpoint_spec {
                if let Some(ports) = endpoint_spec.ports {
                    for port in ports {
                        if let Some(published_port) = port.published_port {
                            let protocol = port.protocol
                                .map(|p| format!("{:?}", p).to_lowercase())
                                .unwrap_or_else(|| "tcp".to_string());
                            let target_port = port.target_port
                                .map(|p| p.to_string())
                                .unwrap_or_else(|| "?".to_string());
                            let publish_mode = port.publish_mode
                                .map(|p| format!("{:?}", p).to_lowercase())
                                .unwrap_or_else(|| "ingress".to_string());
                            
                            used_ports.insert(PortRow {
                                port: published_port.to_string(),
                                protocol,
                                service: service_name.clone(),
                                target_port,
                                publish_mode,
                            });
                        }
                    }
                }
            }
        }
    }
    
    let mut ports_vec: Vec<PortRow> = used_ports.into_iter().collect();
    ports_vec.sort_by(|a, b| {
        let a_port: u16 = a.port.parse().unwrap_or(0);
        let b_port: u16 = b.port.parse().unwrap_or(0);
        a_port.cmp(&b_port)
    });
    Ok(ports_vec)
}

pub async fn list_service_ports(docker: &Docker) -> Result<Vec<ServicePortInfo>> {
    let services = docker
        .list_services(None::<ListServicesOptions>)
        .await?;
    
    let mut all_ports = Vec::new();
    
    for service in services {
        if let Some(spec) = service.spec {
            let service_name = spec.name.unwrap_or_default();
            if let Some(endpoint_spec) = spec.endpoint_spec {
                if let Some(ports) = endpoint_spec.ports {
                    for port in ports {
                        let published_port = port.published_port
                            .map(|p| p as u16)
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "?".to_string());
                        let protocol = port.protocol
                            .map(|p| format!("{:?}", p).to_lowercase())
                            .unwrap_or_else(|| "tcp".to_string());
                        let target_port = port.target_port
                            .map(|p| p as u16)
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "?".to_string());
                        let publish_mode = port.publish_mode
                            .map(|p| format!("{:?}", p).to_lowercase())
                            .unwrap_or_else(|| "ingress".to_string());
                        
                        all_ports.push(ServicePortInfo {
                            service_name: service_name.clone(),
                            published_port,
                            target_port,
                            protocol,
                            publish_mode,
                        });
                    }
                }
            }
        }
    }
    
    all_ports.sort_by(|a, b| {
        let a_port: u16 = a.published_port.parse().unwrap_or(0);
        let b_port: u16 = b.published_port.parse().unwrap_or(0);
        a_port.cmp(&b_port)
    });
    
    Ok(all_ports)
}

pub async fn get_available_ports(docker: &Docker, start: u16, end: u16, protocol: Option<&str>) -> Result<Vec<u16>> {
    let used_ports = list_used_ports(docker).await?;
    
    let filtered_used: HashSet<u16> = used_ports.iter()
        .filter(|p| {
            if let Some(proto) = protocol {
                p.protocol.to_lowercase() == proto.to_lowercase()
            } else {
                true
            }
        })
        .filter_map(|p| p.port.parse().ok())
        .collect();
    
    let available: Vec<u16> = (start..=end)
        .filter(|p| !filtered_used.contains(p))
        .collect();
    
    Ok(available)
}

pub async fn get_port_summary(docker: &Docker) -> Result<PortSummary> {
    let services = docker
        .list_services(None::<ListServicesOptions>)
        .await?;
    
    let mut used_tcp: Vec<u16> = Vec::new();
    let mut used_udp: Vec<u16> = Vec::new();
    let mut port_mappings: Vec<(String, u16, u16, String, String)> = Vec::new();
    
    for service in services {
        if let Some(spec) = service.spec {
            let service_name = spec.name.unwrap_or_default();
            if let Some(endpoint_spec) = spec.endpoint_spec {
                if let Some(ports) = endpoint_spec.ports {
                    for port in ports {
                        if let Some(published_port_i64) = port.published_port {
                            let published_port = published_port_i64 as u16;
                            let proto = port.protocol
                                .map(|p| format!("{:?}", p).to_lowercase())
                                .unwrap_or_else(|| "tcp".to_string());
                            let target_port = port.target_port.unwrap_or(0) as u16;
                            let publish_mode = port.publish_mode
                                .map(|p| format!("{:?}", p).to_lowercase())
                                .unwrap_or_else(|| "ingress".to_string());
                            
                            match proto.as_str() {
                                "udp" => used_udp.push(published_port),
                                _ => used_tcp.push(published_port),
                            }
                            
                            port_mappings.push((service_name.clone(), published_port, target_port, proto, publish_mode));
                        }
                    }
                }
            }
        }
    }
    
    used_tcp.sort();
    used_udp.sort();
    port_mappings.sort_by(|a, b| a.1.cmp(&b.1));
    
    Ok(PortSummary {
        used_tcp,
        used_udp,
        port_mappings,
    })
}

#[derive(Debug, Clone)]
pub struct PortSummary {
    pub used_tcp: Vec<u16>,
    pub used_udp: Vec<u16>,
    pub port_mappings: Vec<(String, u16, u16, String, String)>,
}
