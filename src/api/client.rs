use bollard::{API_DEFAULT_VERSION, Docker};
use std::{env, path::Path};

#[derive(Debug, Clone, Default)]
pub struct ConnectionConfig {
    pub host: Option<String>,
    pub tlscacert: Option<String>,
    pub tlscert: Option<String>,
    pub tlskey: Option<String>,
}

impl ConnectionConfig {
    #[allow(dead_code)]
    pub fn from_env() -> Self {
        Self {
            host: env::var("DOCKER_HOST").ok(),
            ..Default::default()
        }
    }

    pub fn has_tls(&self) -> bool {
        self.tlscacert.is_some() || self.tlscert.is_some() || self.tlskey.is_some()
    }
}

pub struct DockerClient {
    docker: Docker,
}

impl DockerClient {
    #[allow(dead_code)]
    pub fn new() -> Self {
        let docker = Self::connect(&ConnectionConfig::from_env())
            .expect("Failed to connect to Docker daemon");
        Self { docker }
    }

    pub fn with_config(config: &ConnectionConfig) -> anyhow::Result<Self> {
        let docker = Self::connect(config)?;
        Ok(Self { docker })
    }

    pub fn with_context(context_name: Option<&str>) -> anyhow::Result<Self> {
        let config = Self::resolve_config(context_name)?;
        Self::with_config(&config)
    }

    fn resolve_config(context_name: Option<&str>) -> anyhow::Result<ConnectionConfig> {
        if let Some(name) = context_name {
            let ctx = crate::api::context::get_context(name)?;
            return Ok(ctx.to_connection_config());
        }

        if let Some(host) = env::var("DOCKER_HOST").ok() {
            return Ok(ConnectionConfig {
                host: Some(host),
                ..Default::default()
            });
        }

        if let Ok(Some(ctx)) = crate::api::context::get_active_context() {
            log::info!("Using Docker context: {}", ctx.name);
            return Ok(ctx.to_connection_config());
        }

        Ok(ConnectionConfig::default())
    }

    fn connect(config: &ConnectionConfig) -> Result<Docker, bollard::errors::Error> {
        if let Some(host) = &config.host {
            let host = host.trim();
            if host.starts_with("ssh://") {
                return Docker::connect_with_ssh(host, 120, API_DEFAULT_VERSION, None);
            }
            if host.starts_with("unix://") {
                return Docker::connect_with_unix(host, 120, API_DEFAULT_VERSION);
            }
            if config.has_tls() {
                return Docker::connect_with_ssl(
                    host,
                    config
                        .tlscacert
                        .as_deref()
                        .map(Path::new)
                        .unwrap_or_else(|| Path::new("")),
                    config
                        .tlscert
                        .as_deref()
                        .map(Path::new)
                        .unwrap_or_else(|| Path::new("")),
                    config
                        .tlskey
                        .as_deref()
                        .map(Path::new)
                        .unwrap_or_else(|| Path::new("")),
                    120,
                    API_DEFAULT_VERSION,
                );
            }
            return Docker::connect_with_http(host, 120, API_DEFAULT_VERSION);
        }

        // Auto-detect TLS from Docker environment variables
        if env::var("DOCKER_TLS_VERIFY").as_deref() == Ok("1") {
            let cert_path = env::var("DOCKER_CERT_PATH").unwrap_or_else(|_| {
                let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
                format!("{}/.docker", home)
            });
            let host =
                env::var("DOCKER_HOST").unwrap_or_else(|_| "tcp://127.0.0.1:2376".to_string());
            return Docker::connect_with_ssl(
                &host,
                Path::new(&format!("{}/ca.pem", cert_path)),
                Path::new(&format!("{}/cert.pem", cert_path)),
                Path::new(&format!("{}/key.pem", cert_path)),
                120,
                API_DEFAULT_VERSION,
            );
        }

        Docker::connect_with_local_defaults()
    }

    pub fn inner(&self) -> &Docker {
        &self.docker
    }
}
