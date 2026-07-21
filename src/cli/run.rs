use crate::api::client::DockerClient;
use bollard::models::{
    EndpointPortConfig, EndpointPortConfigProtocolEnum, EndpointPortConfigPublishModeEnum,
    EndpointSpec, ServiceSpec, ServiceSpecMode, ServiceSpecModeReplicated, TaskSpec,
    TaskSpecContainerSpec,
};
use std::collections::HashMap;

pub async fn run(
    client: &DockerClient,
    name: String,
    image: String,
    replicas: u64,
    env: Vec<String>,
    labels: Vec<String>,
    _network: Option<String>,
    publish: Vec<String>,
) -> anyhow::Result<()> {
    let env_vars = if env.is_empty() { vec![] } else { env };

    let mut label_map: HashMap<String, String> = HashMap::new();
    for l in &labels {
        if let Some((k, v)) = l.split_once('=') {
            label_map.insert(k.to_string(), v.to_string());
        }
    }

    let mut ports: Vec<EndpointPortConfig> = vec![];
    for p in &publish {
        let parts: Vec<&str> = p.splitn(2, ':').collect();
        let (published_port, target_port) = if parts.len() == 2 {
            (parts[0].parse::<u16>().ok(), parts[1].parse::<u16>().ok())
        } else {
            (p.parse::<u16>().ok(), p.parse::<u16>().ok())
        };
        if let Some(target) = target_port {
            ports.push(EndpointPortConfig {
                published_port: published_port.map(|p| p as i64),
                target_port: Some(target as i64),
                protocol: Some(EndpointPortConfigProtocolEnum::TCP),
                publish_mode: Some(EndpointPortConfigPublishModeEnum::INGRESS),
                name: Some(format!("{}-{}", name, target)),
            });
        }
    }

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
                env: if env_vars.is_empty() {
                    None
                } else {
                    Some(env_vars)
                },
                ..Default::default()
            }),
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
    println!(
        "Service '{}' created (ID: {})",
        name,
        result.id.unwrap_or_default()
    );
    Ok(())
}
