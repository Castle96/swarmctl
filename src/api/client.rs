use bollard::Docker;

pub struct DockerClient {
    docker: Docker,
}

impl DockerClient {
    pub fn new() -> Self {
        let docker = if let Ok(host) = std::env::var("DOCKER_HOST") {
            Docker::connect_with_http(&host, 60, bollard::API_DEFAULT_VERSION)
                .expect("Failed to connect to Docker")
        } else {
            Docker::connect_with_local_defaults().unwrap()
        };

        Self { docker }
    }

    pub fn inner(&self) -> &Docker {
        &self.docker
    }
}