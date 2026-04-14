use crate::api::client::DockerClient;
use crate::api::stack as stack_api;
use anyhow::Context;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bollard::models::{
    ConfigSpec, EndpointPortConfig, EndpointPortConfigProtocolEnum, EndpointSpec,
    NetworkAttachmentConfig, SecretSpec, ServiceSpec, ServiceSpecMode, ServiceSpecModeReplicated,
    TaskSpec, TaskSpecContainerSpec, TaskSpecContainerSpecConfigs, TaskSpecContainerSpecFile,
    TaskSpecContainerSpecFile1, TaskSpecContainerSpecSecrets,
};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;

#[derive(Debug, Deserialize)]
struct ComposeFile {
    services: BTreeMap<String, ComposeService>,
    networks: Option<BTreeMap<String, ComposeNetwork>>,
    configs: Option<BTreeMap<String, ComposeConfig>>,
    secrets: Option<BTreeMap<String, ComposeSecret>>,
}

#[derive(Debug, Deserialize)]
struct ComposeService {
    image: Option<String>,
    command: Option<StringOrVec>,
    args: Option<Vec<String>>,
    environment: Option<EnvList>,
    ports: Option<Vec<String>>,
    networks: Option<ComposeServiceNetworks>,
    labels: Option<HashMap<String, String>>,
    deploy: Option<ComposeDeploy>,
    configs: Option<Vec<ComposeServiceConfig>>,
    secrets: Option<Vec<ComposeServiceSecret>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum StringOrVec {
    String(String),
    Vec(Vec<String>),
}

impl StringOrVec {
    fn into_vec(self) -> Vec<String> {
        match self {
            StringOrVec::String(value) => vec![value],
            StringOrVec::Vec(value) => value,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum EnvList {
    Map(HashMap<String, String>),
    List(Vec<String>),
}

impl EnvList {
    fn into_vec(self) -> Vec<String> {
        match self {
            EnvList::Map(map) => map
                .into_iter()
                .map(|(key, value)| format!("{}={}", key, value))
                .collect(),
            EnvList::List(list) => list,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ComposeServiceNetworks {
    List(Vec<String>),
    Map(HashMap<String, ComposeNetworkAttachment>),
}

#[derive(Debug, Deserialize)]
struct ComposeNetworkAttachment {
    aliases: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ComposeNetwork {
    driver: Option<String>,
    internal: Option<bool>,
    labels: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct ComposeDeploy {
    replicas: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ComposeServiceConfig {
    Name(String),
    Detailed {
        source: String,
        target: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ComposeServiceSecret {
    Name(String),
    Detailed {
        source: String,
        target: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct ComposeConfig {
    file: Option<String>,
    external: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ComposeSecret {
    file: Option<String>,
    external: Option<bool>,
}

pub async fn deploy(
    client: &DockerClient,
    compose_file: String,
    stack_name: String,
) -> anyhow::Result<()> {
    let content = fs::read_to_string(&compose_file)
        .with_context(|| format!("Failed to read compose file {}", compose_file))?;
    let compose: ComposeFile = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse compose file {}", compose_file))?;

    create_stack_resources(client.inner(), &compose, &stack_name).await
}

pub async fn list(client: &DockerClient) -> anyhow::Result<()> {
    let stacks = stack_api::list_stacks(client.inner()).await?;

    if stacks.is_empty() {
        println!("No stacks found");
        return Ok(());
    }

    println!("NAME\tSERVICES\tREPLICAS");
    for stack in stacks {
        println!("{}\t{}\t{}", stack.name, stack.services, stack.replicas);
    }

    Ok(())
}

pub async fn remove(client: &DockerClient, name: &str) -> anyhow::Result<()> {
    stack_api::remove_stack(client.inner(), name).await?;
    println!("Stack {} removed", name);
    Ok(())
}

async fn create_stack_resources(
    docker: &bollard::Docker,
    compose: &ComposeFile,
    stack_name: &str,
) -> anyhow::Result<()> {
    create_networks(docker, compose.networks.as_ref(), stack_name).await?;
    let config_map = create_configs(docker, compose.configs.as_ref(), stack_name).await?;
    let secret_map = create_secrets(docker, compose.secrets.as_ref(), stack_name).await?;
    create_services(
        docker,
        &compose.services,
        stack_name,
        &config_map,
        &secret_map,
    )
    .await?;
    Ok(())
}

async fn create_networks(
    docker: &bollard::Docker,
    networks: Option<&BTreeMap<String, ComposeNetwork>>,
    stack_name: &str,
) -> anyhow::Result<()> {
    if let Some(networks) = networks {
        for (network_name, network_def) in networks {
            let full_name = format!("{}_{}", stack_name, network_name);
            let mut labels = HashMap::new();
            labels.insert(
                "com.docker.stack.namespace".to_string(),
                stack_name.to_string(),
            );
            if let Some(def_labels) = &network_def.labels {
                labels.extend(def_labels.clone());
            }

            let exists = network_exists(docker, &full_name).await?;
            if exists {
                println!("Network {} already exists", full_name);
                continue;
            }

            let spec = bollard::models::NetworkCreateRequest {
                name: full_name.clone(),
                internal: network_def.internal,
                labels: Some(labels),
                driver: network_def.driver.clone(),
                ..Default::default()
            };

            let response = docker.create_network(spec).await?;
            println!("Created network {} with ID {}", full_name, response.id);
        }
    }

    Ok(())
}

async fn network_exists(docker: &bollard::Docker, name: &str) -> anyhow::Result<bool> {
    let mut filters = HashMap::new();
    filters.insert("name".to_string(), vec![name.to_string()]);

    let options = bollard::query_parameters::ListNetworksOptions {
        filters: Some(filters),
    };
    let networks = docker.list_networks(Some(options)).await?;
    Ok(!networks.is_empty())
}

async fn create_configs(
    docker: &bollard::Docker,
    configs: Option<&BTreeMap<String, ComposeConfig>>,
    stack_name: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    if let Some(configs) = configs {
        for (config_name, config_def) in configs {
            let actual_name = if config_def.external.unwrap_or(false) {
                config_name.clone()
            } else {
                format!("{}_{}", stack_name, config_name)
            };

            map.insert(config_name.clone(), actual_name.clone());

            if config_def.external.unwrap_or(false) {
                println!("Skipping external config {}", actual_name);
                continue;
            }

            if config_exists(docker, &actual_name).await? {
                println!("Config {} already exists", actual_name);
                continue;
            }

            let file = config_def
                .file
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Config {} requires a file path", config_name))?;
            let data =
                fs::read(file).with_context(|| format!("Failed to read config file {}", file))?;
            let data_b64 = STANDARD.encode(&data);

            let mut labels = HashMap::new();
            labels.insert(
                "com.docker.stack.namespace".to_string(),
                stack_name.to_string(),
            );

            let spec = ConfigSpec {
                name: Some(actual_name.clone()),
                data: Some(data_b64),
                labels: Some(labels),
                ..Default::default()
            };

            let response = docker.create_config(spec).await?;
            println!("Created config {} with ID {}", actual_name, response.id);
        }
    }
    Ok(map)
}

async fn config_exists(docker: &bollard::Docker, name: &str) -> anyhow::Result<bool> {
    let mut filters = HashMap::new();
    filters.insert("name".to_string(), vec![name.to_string()]);
    let options = bollard::query_parameters::ListConfigsOptions {
        filters: Some(filters),
    };
    let configs = docker.list_configs(Some(options)).await?;
    Ok(!configs.is_empty())
}

async fn create_secrets(
    docker: &bollard::Docker,
    secrets: Option<&BTreeMap<String, ComposeSecret>>,
    stack_name: &str,
) -> anyhow::Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    if let Some(secrets) = secrets {
        for (secret_name, secret_def) in secrets {
            let actual_name = if secret_def.external.unwrap_or(false) {
                secret_name.clone()
            } else {
                format!("{}_{}", stack_name, secret_name)
            };

            map.insert(secret_name.clone(), actual_name.clone());

            if secret_def.external.unwrap_or(false) {
                println!("Skipping external secret {}", actual_name);
                continue;
            }

            if secret_exists(docker, &actual_name).await? {
                println!("Secret {} already exists", actual_name);
                continue;
            }

            let file = secret_def
                .file
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Secret {} requires a file path", secret_name))?;
            let data =
                fs::read(file).with_context(|| format!("Failed to read secret file {}", file))?;
            let data_b64 = STANDARD.encode(&data);

            let mut labels = HashMap::new();
            labels.insert(
                "com.docker.stack.namespace".to_string(),
                stack_name.to_string(),
            );

            let spec = SecretSpec {
                name: Some(actual_name.clone()),
                data: Some(data_b64),
                labels: Some(labels),
                ..Default::default()
            };

            let response = docker.create_secret(spec).await?;
            println!("Created secret {} with ID {}", actual_name, response.id);
        }
    }
    Ok(map)
}

async fn secret_exists(docker: &bollard::Docker, name: &str) -> anyhow::Result<bool> {
    let mut filters = HashMap::new();
    filters.insert("name".to_string(), vec![name.to_string()]);
    let options = bollard::query_parameters::ListSecretsOptions {
        filters: Some(filters),
    };
    let secrets = docker.list_secrets(Some(options)).await?;
    Ok(!secrets.is_empty())
}

async fn create_services(
    docker: &bollard::Docker,
    services: &BTreeMap<String, ComposeService>,
    stack_name: &str,
    config_map: &HashMap<String, String>,
    secret_map: &HashMap<String, String>,
) -> anyhow::Result<()> {
    for (name, service) in services {
        let full_service_name = format!("{}_{}", stack_name, name);
        let mut labels = service.labels.clone().unwrap_or_default();
        labels.insert(
            "com.docker.stack.namespace".to_string(),
            stack_name.to_string(),
        );

        let env = service
            .environment
            .as_ref()
            .map(|env| env.clone().into_vec())
            .unwrap_or_default();

        let configs = compose_configs(service.configs.as_ref(), stack_name, config_map);
        let secrets = compose_secrets(service.secrets.as_ref(), stack_name, secret_map);
        let endpoint_spec = compose_ports(service.ports.as_ref())?;

        let container_spec = TaskSpecContainerSpec {
            image: service.image.clone(),
            command: service.command.as_ref().map(|c| c.clone().into_vec()),
            args: service.args.clone(),
            env: if env.is_empty() { None } else { Some(env) },
            configs,
            secrets,
            ..Default::default()
        };

        let task_spec = TaskSpec {
            container_spec: Some(container_spec),
            ..Default::default()
        };

        let mode = service.deploy.as_ref().and_then(|deploy| {
            deploy.replicas.map(|replicas| ServiceSpecMode {
                replicated: Some(ServiceSpecModeReplicated {
                    replicas: Some(replicas.try_into().unwrap_or(i64::MAX)),
                }),
                ..Default::default()
            })
        });

        let networks = compose_networks(service.networks.as_ref(), stack_name);

        let spec = ServiceSpec {
            name: Some(full_service_name.clone()),
            labels: Some(labels),
            task_template: Some(task_spec),
            mode,
            networks,
            endpoint_spec,
            ..Default::default()
        };

        let response = docker.create_service(spec, None).await?;
        println!(
            "Created service {} with ID {}",
            full_service_name,
            response.id.unwrap_or_default()
        );
    }

    Ok(())
}

fn compose_networks(
    networks: Option<&ComposeServiceNetworks>,
    stack_name: &str,
) -> Option<Vec<NetworkAttachmentConfig>> {
    let attachments = match networks {
        Some(ComposeServiceNetworks::List(list)) => list
            .iter()
            .map(|network_name| NetworkAttachmentConfig {
                target: Some(format!("{}_{}", stack_name, network_name)),
                ..Default::default()
            })
            .collect(),
        Some(ComposeServiceNetworks::Map(map)) => map
            .iter()
            .map(|(network_name, attachment)| NetworkAttachmentConfig {
                target: Some(format!("{}_{}", stack_name, network_name)),
                aliases: attachment.aliases.clone(),
                ..Default::default()
            })
            .collect(),
        None => return None,
    };

    Some(attachments)
}

fn compose_configs(
    configs: Option<&Vec<ComposeServiceConfig>>,
    stack_name: &str,
    config_map: &HashMap<String, String>,
) -> Option<Vec<TaskSpecContainerSpecConfigs>> {
    configs.map(|configs| {
        configs
            .iter()
            .filter_map(|config| {
                let (source, target) = match config {
                    ComposeServiceConfig::Name(name) => (name.clone(), None),
                    ComposeServiceConfig::Detailed { source, target } => {
                        (source.clone(), target.clone())
                    }
                };
                let actual_name = config_map
                    .get(&source)
                    .cloned()
                    .unwrap_or_else(|| format!("{}_{}", stack_name, source));
                let file = target.map(|target| TaskSpecContainerSpecFile1 {
                    name: Some(target),
                    uid: None,
                    gid: None,
                    mode: None,
                });

                Some(TaskSpecContainerSpecConfigs {
                    config_id: None,
                    config_name: Some(actual_name),
                    file,
                    runtime: None,
                })
            })
            .collect()
    })
}

fn compose_secrets(
    secrets: Option<&Vec<ComposeServiceSecret>>,
    stack_name: &str,
    secret_map: &HashMap<String, String>,
) -> Option<Vec<TaskSpecContainerSpecSecrets>> {
    secrets.map(|secrets| {
        secrets
            .iter()
            .filter_map(|secret| {
                let (source, target) = match secret {
                    ComposeServiceSecret::Name(name) => (name.clone(), None),
                    ComposeServiceSecret::Detailed { source, target } => {
                        (source.clone(), target.clone())
                    }
                };
                let actual_name = secret_map
                    .get(&source)
                    .cloned()
                    .unwrap_or_else(|| format!("{}_{}", stack_name, source));
                let file = target.map(|target| TaskSpecContainerSpecFile {
                    name: Some(target),
                    uid: None,
                    gid: None,
                    mode: None,
                });

                Some(TaskSpecContainerSpecSecrets {
                    file,
                    secret_id: None,
                    secret_name: Some(actual_name),
                })
            })
            .collect()
    })
}

fn compose_ports(ports: Option<&Vec<String>>) -> anyhow::Result<Option<EndpointSpec>> {
    if let Some(ports) = ports {
        let port_configs: anyhow::Result<Vec<EndpointPortConfig>> = ports
            .iter()
            .map(|value| parse_port_mapping(value.as_str()))
            .collect();

        let port_configs = port_configs?;
        if port_configs.is_empty() {
            return Ok(None);
        }

        return Ok(Some(EndpointSpec {
            ports: Some(port_configs),
            ..Default::default()
        }));
    }

    Ok(None)
}

fn parse_port_mapping(value: &str) -> anyhow::Result<EndpointPortConfig> {
    let value = value.trim();
    let published;
    let target;
    let mut protocol = EndpointPortConfigProtocolEnum::TCP;

    let parts: Vec<&str> = value.split(':').collect();
    match parts.as_slice() {
        [single] => {
            let port = single.parse::<i64>()?;
            published = Some(port);
            target = Some(port);
        }
        [published_str, target_str] => {
            published = Some(published_str.parse::<i64>()?);
            let target_and_proto: Vec<&str> = target_str.split('/').collect();
            target = Some(target_and_proto[0].parse::<i64>()?);
            if let Some(proto) = target_and_proto.get(1) {
                protocol = match proto.to_lowercase().as_str() {
                    "udp" => EndpointPortConfigProtocolEnum::UDP,
                    _ => EndpointPortConfigProtocolEnum::TCP,
                };
            }
        }
        [_, _, _] => {
            published = Some(parts[1].parse::<i64>()?);
            target = Some(parts[2].parse::<i64>()?);
        }
        _ => return Err(anyhow::anyhow!("Invalid port mapping: {}", value)),
    }

    Ok(EndpointPortConfig {
        published_port: published,
        target_port: target,
        protocol: Some(protocol),
        ..Default::default()
    })
}

// Docker stack deploy compatibility includes services, networks, configs, and secrets.
