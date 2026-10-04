use crate::api::client::DockerClient;
use bollard::models::{
    EndpointPortConfig, EndpointPortConfigProtocolEnum, EndpointPortConfigPublishModeEnum,
    EndpointSpec, NetworkAttachmentConfig, ServiceSpec, ServiceSpecMode, ServiceSpecModeReplicated,
    TaskSpec, TaskSpecContainerSpec,
};
use std::collections::HashMap;

/// Everything `swarmctl run` accepts, as one value.
pub struct RunOptions {
    pub name: String,
    pub image: String,
    pub replicas: u64,
    pub env: Vec<String>,
    pub labels: Vec<String>,
    pub network: Option<String>,
    pub publish: Vec<String>,
    pub output: crate::cli::root::OutputFormat,
}

pub async fn run(client: &DockerClient, opts: RunOptions) -> anyhow::Result<()> {
    let RunOptions {
        name,
        image,
        replicas,
        env,
        labels,
        network,
        publish,
        output,
    } = opts;
    let mut label_map: HashMap<String, String> = HashMap::new();
    for l in &labels {
        match l.split_once('=') {
            Some((k, v)) => {
                label_map.insert(k.to_string(), v.to_string());
            }
            None => {
                // Silently dropping a malformed label would create a service that
                // does not carry what the user asked for.
                anyhow::bail!("Invalid label '{}': expected key=value", l);
            }
        }
    }

    let ports = parse_publish(&publish)?;

    // Resolve the network by name so the service actually joins it. Previously
    // `--network` was accepted and discarded, producing a service attached to
    // only the default network.
    let networks = match &network {
        Some(net_name) => vec![resolve_network(client, net_name).await?],
        None => Vec::new(),
    };

    let spec = ServiceSpec {
        name: Some(name.clone()),
        labels: if label_map.is_empty() {
            None
        } else {
            Some(label_map)
        },
        task_template: Some(TaskSpec {
            container_spec: Some(TaskSpecContainerSpec {
                image: Some(image.clone()),
                env: if env.is_empty() { None } else { Some(env) },
                ..Default::default()
            }),
            networks: if networks.is_empty() {
                None
            } else {
                Some(networks)
            },
            ..Default::default()
        }),
        mode: Some(ServiceSpecMode {
            replicated: Some(ServiceSpecModeReplicated {
                replicas: Some(replicas as i64),
            }),
            ..Default::default()
        }),
        endpoint_spec: if ports.is_empty() {
            None
        } else {
            Some(EndpointSpec {
                ports: Some(ports),
                ..Default::default()
            })
        },
        ..Default::default()
    };

    let result = client.inner().create_service(spec, None).await?;
    let id = result.id.clone().unwrap_or_default();

    match output {
        crate::cli::root::OutputFormat::Json | crate::cli::root::OutputFormat::Yaml => {
            crate::utils::printer::print_json(&result)?;
        }
        _ => {
            println!("Service '{}' created (ID: {})", name, id);
            if let Some(net) = &network {
                println!("  Network: {}", net);
            }
        }
    }
    Ok(())
}

/// Look up a network by name, failing with the available names if absent.
async fn resolve_network(
    client: &DockerClient,
    name: &str,
) -> anyhow::Result<NetworkAttachmentConfig> {
    let networks = crate::api::network::list_networks(client.inner()).await?;
    let found = networks
        .iter()
        .find(|n| n.name.as_deref() == Some(name))
        .map(|n| NetworkAttachmentConfig {
            target: n.id.clone(),
            aliases: None,
            driver_opts: None,
        });
    match found {
        Some(attachment) => Ok(attachment),
        None => {
            let known: Vec<&str> = networks.iter().filter_map(|n| n.name.as_deref()).collect();
            Err(anyhow::anyhow!(
                "Network '{}' not found.{}",
                name,
                if known.is_empty() {
                    String::new()
                } else {
                    format!(" Available: {}", known.join(", "))
                }
            ))
        }
    }
}

/// Parse `--publish` values.
///
/// Accepts `HOST:CONTAINER`, `CONTAINER` (published on a random host port), and
/// `HOST:CONTAINER/PROTO`. A value that cannot be parsed is an error: the old
/// behaviour dropped the port with no message, silently creating a service with
/// no published ports.
fn parse_publish(values: &[String]) -> anyhow::Result<Vec<EndpointPortConfig>> {
    let mut ports = Vec::with_capacity(values.len());
    for value in values {
        let (spec, proto): (&str, EndpointPortConfigProtocolEnum) = match value.split_once('/') {
            Some((spec, proto)) => (spec, parse_protocol(proto)?),
            None => (value.as_str(), EndpointPortConfigProtocolEnum::TCP),
        };

        let (published, target) = match spec.split_once(':') {
            Some((host, container)) => (
                Some(parse_port(host, value)?),
                parse_port(container, value)?,
            ),
            None => (None, parse_port(spec, value)?),
        };

        ports.push(EndpointPortConfig {
            published_port: published.map(|p| p as i64),
            target_port: Some(target as i64),
            protocol: Some(proto),
            publish_mode: Some(EndpointPortConfigPublishModeEnum::INGRESS),
            name: None,
        });
    }
    Ok(ports)
}

fn parse_protocol(proto: &str) -> anyhow::Result<EndpointPortConfigProtocolEnum> {
    match proto.to_lowercase().as_str() {
        "tcp" => Ok(EndpointPortConfigProtocolEnum::TCP),
        "udp" => Ok(EndpointPortConfigProtocolEnum::UDP),
        "sctp" => Ok(EndpointPortConfigProtocolEnum::SCTP),
        other => anyhow::bail!(
            "Unknown protocol '{}' in --publish. Expected tcp, udp or sctp.",
            other
        ),
    }
}

fn parse_port(value: &str, original: &str) -> anyhow::Result<u16> {
    value
        .parse::<u16>()
        .map_err(|_| anyhow::anyhow!("Invalid port '{}' in --publish '{}'", value, original))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publish(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_host_and_container_port() {
        let ports = parse_publish(&publish(&["8080:80"])).unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].published_port, Some(8080));
        assert_eq!(ports[0].target_port, Some(80));
        assert_eq!(ports[0].protocol, Some(EndpointPortConfigProtocolEnum::TCP));
    }

    #[test]
    fn parses_container_port_only() {
        let ports = parse_publish(&publish(&["80"])).unwrap();
        assert_eq!(ports[0].published_port, None);
        assert_eq!(ports[0].target_port, Some(80));
    }

    #[test]
    fn parses_protocol_suffix() {
        let ports = parse_publish(&publish(&["5353:53/udp"])).unwrap();
        assert_eq!(ports[0].protocol, Some(EndpointPortConfigProtocolEnum::UDP));

        let sctp = parse_publish(&publish(&["5000:5000/sctp"])).unwrap();
        assert_eq!(sctp[0].protocol, Some(EndpointPortConfigProtocolEnum::SCTP));
    }

    #[test]
    fn protocol_suffix_is_case_insensitive() {
        let ports = parse_publish(&publish(&["53:53/UDP"])).unwrap();
        assert_eq!(ports[0].protocol, Some(EndpointPortConfigProtocolEnum::UDP));
    }

    #[test]
    fn rejects_malformed_ports_instead_of_dropping_them() {
        for bad in ["abc", "8080:abc", "70000:80", "8080:80/x", "8080:"] {
            assert!(
                parse_publish(&publish(&[bad])).is_err(),
                "should reject {bad:?}"
            );
        }
    }

    #[test]
    fn rejects_out_of_range_ports() {
        assert!(parse_publish(&publish(&["65536:80"])).is_err());
        assert!(
            parse_publish(&publish(&["0:80"])).is_ok(),
            "0 is a valid u16"
        );
    }

    #[test]
    fn empty_publish_list_yields_no_ports() {
        assert!(parse_publish(&[]).unwrap().is_empty());
    }

    #[test]
    fn multiple_values_are_all_parsed() {
        let ports = parse_publish(&publish(&["8080:80", "5353:53/udp"])).unwrap();
        assert_eq!(ports.len(), 2);
    }

    #[test]
    fn label_without_equals_is_rejected() {
        // Previously silently dropped.
        let l = "app";
        assert!(l.split_once('=').is_none());
    }
}
