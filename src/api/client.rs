use bollard::{API_DEFAULT_VERSION, Docker};
use std::{env, path::Path};

#[derive(Debug, Clone, Default)]
pub struct ConnectionConfig {
    pub host: Option<String>,
    pub tlscacert: Option<String>,
    pub tlscert: Option<String>,
    pub tlskey: Option<String>,
    /// Mirrors Docker's `SkipTLSVerify`. Bollard always verifies the server
    /// certificate and exposes no way to opt out, so this is carried only so we
    /// can warn instead of silently ignoring it.
    pub skip_tls_verify: bool,
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
        Self::preflight(config)?;
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

        // Match Docker's own precedence: an explicitly selected context wins
        // over DOCKER_HOST. Previously DOCKER_HOST was checked first, so with
        // both set swarmctl could talk to a different daemon than `docker`.
        if let Ok(name) = env::var("DOCKER_CONTEXT")
            && !name.is_empty()
        {
            log::info!("Using Docker context from DOCKER_CONTEXT: {}", name);
            return Ok(crate::api::context::get_context(&name)?.to_connection_config());
        }

        if let Ok(host) = env::var("DOCKER_HOST") {
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

    /// Reject endpoints we cannot honour, instead of silently degrading.
    fn preflight(config: &ConnectionConfig) -> anyhow::Result<()> {
        if config.skip_tls_verify {
            anyhow::bail!(
                "This endpoint requests SkipTLSVerify, but swarmctl always verifies the \
                 server certificate. Use `ssh://` or provide a CA certificate via \
                 --tlscacert."
            );
        }
        if config.has_tls() {
            let missing: Vec<&str> = [
                ("--tlscacert", config.tlscacert.is_none()),
                ("--tlscert", config.tlscert.is_none()),
                ("--tlskey", config.tlskey.is_none()),
            ]
            .into_iter()
            .filter_map(|(flag, is_missing)| is_missing.then_some(flag))
            .collect();
            if !missing.is_empty() {
                anyhow::bail!(
                    "Incomplete TLS configuration: {} not specified. Docker requires the CA, \
                     client certificate and client key together.",
                    missing.join(", ")
                );
            }
        }
        Ok(())
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
                // preflight() has already rejected partial TLS configuration.
                let ca = Path::new(config.tlscacert.as_deref().unwrap_or_default());
                let cert = Path::new(config.tlscert.as_deref().unwrap_or_default());
                let key = Path::new(config.tlskey.as_deref().unwrap_or_default());
                return Docker::connect_with_ssl(host, key, cert, ca, 120, API_DEFAULT_VERSION);
            }
            return Docker::connect_with_http(host, 120, API_DEFAULT_VERSION);
        }

        // Auto-detect TLS from Docker environment variables
        if env::var("DOCKER_TLS_VERIFY").as_deref() == Ok("1") {
            let cert_path = env::var("DOCKER_CERT_PATH").unwrap_or_else(|_| {
                crate::api::docker_check::docker_config_dir()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| ".docker".to_string())
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
