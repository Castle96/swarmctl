use bollard::{API_DEFAULT_VERSION, Docker};
use std::{env, path::Path};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
    pub fn has_tls(&self) -> bool {
        self.tlscacert.is_some() || self.tlscert.is_some() || self.tlskey.is_some()
    }

    /// Which TLS flags are set but incomplete.
    ///
    /// Docker requires the CA, client certificate and client key together.
    /// Returns the names of the missing flags so the error can name them.
    fn missing_tls_flags(&self) -> Vec<&'static str> {
        [
            ("--tlscacert", self.tlscacert.is_none()),
            ("--tlscert", self.tlscert.is_none()),
            ("--tlskey", self.tlskey.is_none()),
        ]
        .into_iter()
        .filter_map(|(flag, is_missing)| is_missing.then_some(flag))
        .collect()
    }

    /// Which transport `connect` will choose, for a given host.
    ///
    /// Pure, so the scheme precedence can be tested without dialling anything.
    fn transport(&self, host: &str) -> Transport {
        let host = host.trim();
        if host.starts_with("ssh://") {
            return Transport::Ssh;
        }
        if host.starts_with("unix://") {
            return Transport::Unix;
        }
        if self.has_tls() {
            return Transport::Tls;
        }
        Transport::Http
    }
}

/// The transport `connect` will dial for a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    Ssh,
    Unix,
    Tls,
    /// Plain HTTP, including any `https://` scheme.
    ///
    /// Docker does not accept `https://` in `DOCKER_HOST`, so treating it as
    /// plain HTTP matches Docker's own behaviour rather than silently
    /// upping the connection to TLS.
    Http,
}

/// A connected Docker daemon.
///
/// Cheap to clone: `Docker` is itself an `Arc` over the hyper connector, so a
/// clone shares the connection pool rather than dialling again.
#[derive(Clone)]
pub struct DockerClient {
    docker: Docker,
}

impl DockerClient {
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
        Self::resolve_config_with(
            context_name,
            env::var("DOCKER_CONTEXT").ok().as_deref(),
            env::var("DOCKER_HOST").ok().as_deref(),
            || Ok(crate::api::context::get_active_context()?.map(|ctx| ctx.to_connection_config())),
        )
    }

    /// The connection precedence chain, as a pure function of its arguments.
    ///
    /// Split out from [`Self::resolve_config`] so the ordering can be tested
    /// without mutating the process environment. The Docker-context lookups
    /// read `$HOME`, so a test that uses one must set `HOME` to a temp
    /// directory; the precedence itself needs no such setup.
    fn resolve_config_with(
        context_name: Option<&str>,
        docker_context: Option<&str>,
        docker_host: Option<&str>,
        active_context: impl FnOnce() -> anyhow::Result<Option<ConnectionConfig>>,
    ) -> anyhow::Result<ConnectionConfig> {
        if let Some(name) = context_name {
            return Ok(crate::api::context::get_context(name)?.to_connection_config());
        }

        // Match Docker's own precedence: an explicitly selected context wins
        // over DOCKER_HOST. Previously DOCKER_HOST was checked first, so with
        // both set swarmctl could talk to a different daemon than `docker`.
        if let Some(name) = docker_context.filter(|n| !n.is_empty()) {
            log::info!("Using Docker context from DOCKER_CONTEXT: {}", name);
            return Ok(crate::api::context::get_context(name)?.to_connection_config());
        }

        if let Some(host) = docker_host {
            log::info!("Using DOCKER_HOST: {}", host);
            return Ok(ConnectionConfig {
                host: Some(host.to_string()),
                ..Default::default()
            });
        }

        if let Some(config) = active_context()? {
            return Ok(config);
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
            let missing = config.missing_tls_flags();
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
            match config.transport(host) {
                Transport::Ssh => {
                    return Docker::connect_with_ssh(host, 120, API_DEFAULT_VERSION, None);
                }
                Transport::Unix => {
                    return Docker::connect_with_unix(host, 120, API_DEFAULT_VERSION);
                }
                Transport::Tls => {
                    // preflight() has already rejected partial TLS configuration,
                    // so all three are present here.
                    let ca = Path::new(config.tlscacert.as_deref().unwrap_or_default());
                    let cert = Path::new(config.tlscert.as_deref().unwrap_or_default());
                    let key = Path::new(config.tlskey.as_deref().unwrap_or_default());
                    return Docker::connect_with_ssl(host, key, cert, ca, 120, API_DEFAULT_VERSION);
                }
                Transport::Http => {
                    return Docker::connect_with_http(host, 120, API_DEFAULT_VERSION);
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tls_config() -> ConnectionConfig {
        ConnectionConfig {
            tlscacert: Some("/ca.pem".into()),
            tlscert: Some("/cert.pem".into()),
            tlskey: Some("/key.pem".into()),
            ..Default::default()
        }
    }

    /// The precedence chain is the most consequential logic in the crate and
    /// had no tests at all. `DOCKER_HOST` was previously checked before
    /// `DOCKER_CONTEXT`, so with both set swarmctl could talk to a different
    /// daemon than `docker` does.
    #[test]
    fn context_beats_host_matching_dockers_precedence() {
        let got = DockerClient::resolve_config_with(
            None,
            Some("prod"),
            Some("tcp://from-host:2375"),
            || Ok(None),
        )
        // The context lookup fails against an empty Docker config, which is
        // fine: the point is that it was attempted *first*.
        .expect_err("a context lookup should have been tried before DOCKER_HOST");
        assert!(
            got.to_string().to_lowercase().contains("context"),
            "expected a context error, got: {got}"
        );
    }

    #[test]
    fn host_is_used_when_no_context_is_set() {
        let got =
            DockerClient::resolve_config_with(None, None, Some("tcp://from-host:2375"), || {
                Ok(None)
            })
            .unwrap();
        assert_eq!(got.host.as_deref(), Some("tcp://from-host:2375"));
    }

    #[test]
    fn an_empty_docker_context_is_ignored() {
        // `DOCKER_CONTEXT=` should not send us looking for a context named "".
        let got =
            DockerClient::resolve_config_with(None, Some(""), Some("tcp://from-host:2375"), || {
                Ok(None)
            })
            .unwrap();
        assert_eq!(got.host.as_deref(), Some("tcp://from-host:2375"));
    }

    #[test]
    fn the_active_docker_context_is_used_before_the_local_socket() {
        let active = ConnectionConfig {
            host: Some("ssh://from-context".into()),
            ..Default::default()
        };
        let got = DockerClient::resolve_config_with(None, None, None, || Ok(Some(active))).unwrap();
        assert_eq!(got.host.as_deref(), Some("ssh://from-context"));
    }

    #[test]
    fn nothing_configured_falls_back_to_local_defaults() {
        let got = DockerClient::resolve_config_with(None, None, None, || Ok(None)).unwrap();
        // An empty config makes `connect` call `connect_with_local_defaults`.
        assert_eq!(got, ConnectionConfig::default());
    }

    #[test]
    fn a_context_error_is_propagated_not_swallowed() {
        let err = DockerClient::resolve_config_with(None, None, None, || {
            Err(anyhow::anyhow!("no docker config"))
        })
        .expect_err("a failure reading the active context must not be ignored");
        assert!(err.to_string().contains("no docker config"));
    }

    /// `SkipTLSVerify` cannot be honoured because Bollard always verifies the
    /// certificate. Rejecting it is the whole point of carrying the field.
    #[test]
    fn skip_tls_verify_is_refused_with_guidance() {
        let config = ConnectionConfig {
            skip_tls_verify: true,
            ..tls_config()
        };
        let err = DockerClient::preflight(&config).expect_err("must not be silently ignored");
        let msg = err.to_string();
        assert!(msg.contains("ssh://"), "should suggest ssh: {msg}");
        assert!(msg.contains("--tlscacert"), "should suggest a CA: {msg}");
    }

    #[test]
    fn a_complete_tls_triple_is_accepted() {
        assert!(DockerClient::preflight(&tls_config()).is_ok());
    }

    #[test]
    fn a_no_tls_config_is_accepted() {
        assert!(DockerClient::preflight(&ConnectionConfig::default()).is_ok());
    }

    /// Docker requires all three TLS files together; naming the missing ones
    /// is what makes the error actionable.
    #[test]
    fn partial_tls_config_names_every_missing_flag() {
        let err = DockerClient::preflight(&ConnectionConfig {
            tlscacert: Some("/ca.pem".into()),
            ..Default::default()
        })
        .expect_err("a partial TLS config must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("--tlscert"), "{msg}");
        assert!(msg.contains("--tlskey"), "{msg}");
        assert!(
            !msg.contains("--tlscacert"),
            "the supplied flag should not be reported missing: {msg}"
        );
    }

    #[test]
    fn a_tls_key_without_a_ca_names_the_ca() {
        let err = DockerClient::preflight(&ConnectionConfig {
            tlskey: Some("/key.pem".into()),
            ..Default::default()
        })
        .expect_err("a lone key is not enough");
        let msg = err.to_string();
        assert!(msg.contains("--tlscacert"), "{msg}");
        assert!(msg.contains("--tlscert"), "{msg}");
    }

    /// Scheme dispatch: `ssh://` and `unix://` must not be routed through the
    /// plain-HTTP client, and TLS must win over HTTP when configured.
    #[test]
    fn transport_is_chosen_from_the_scheme() {
        let plain = ConnectionConfig::default();
        assert_eq!(plain.transport("ssh://user@host"), Transport::Ssh);
        assert_eq!(
            plain.transport("unix:///var/run/docker.sock"),
            Transport::Unix
        );
        assert_eq!(plain.transport("tcp://1.2.3.4:2375"), Transport::Http);
        // Surrounding whitespace must not defeat scheme detection.
        assert_eq!(plain.transport("  ssh://user@host  "), Transport::Ssh);
    }

    #[test]
    fn tls_config_selects_the_tls_transport() {
        assert_eq!(tls_config().transport("tcp://1.2.3.4:2376"), Transport::Tls);
        // ssh:// still wins: SSH handles its own transport encryption.
        assert_eq!(tls_config().transport("ssh://user@host"), Transport::Ssh);
        assert_eq!(
            tls_config().transport("unix:///var/run/docker.sock"),
            Transport::Unix
        );
    }

    /// `https://` is not a Docker `DOCKER_HOST` scheme; treating it as plain
    /// HTTP matches Docker rather than silently upgrading the connection.
    #[test]
    fn https_scheme_is_not_upgraded_to_tls() {
        assert_eq!(
            ConnectionConfig::default().transport("https://host:2376"),
            Transport::Http
        );
    }

    #[test]
    fn has_tls_requires_any_one_flag() {
        assert!(!ConnectionConfig::default().has_tls());
        assert!(
            ConnectionConfig {
                tlskey: Some("/k".into()),
                ..Default::default()
            }
            .has_tls()
        );
    }
}
