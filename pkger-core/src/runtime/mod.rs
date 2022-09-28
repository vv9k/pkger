pub mod container;
pub mod docker;
pub mod podman;

pub use docker::DockerContainer;
pub use docker_api;
pub use podman::PodmanContainer;
pub use podman_api;

use crate::{ErrContext, Result};

use docker_api::Docker;
use podman_api::Podman;

#[derive(Clone, Debug)]
pub enum RuntimeConnector {
    Docker(docker_api::Docker),
    Podman(podman_api::Podman),
}

pub struct ConnectionPool {
    connector: RuntimeConnector,
}

impl ConnectionPool {
    pub async fn new_checked(uri: impl Into<String>) -> Result<Self> {
        let uri = uri.into();
        let podman = Podman::new(&uri)?;
        if podman.ping().await.is_ok() {
            return Ok(Self::podman(podman));
        }
        let docker = Docker::new(&uri)?;
        docker
            .ping()
            .await
            .map(|_| Self::docker(docker))
            .context(format!("failed to ping container runtime at `{uri}`"))
    }

    pub fn docker(docker: Docker) -> Self {
        Self {
            connector: RuntimeConnector::Docker(docker),
        }
    }

    pub fn podman(podman: Podman) -> Self {
        Self {
            connector: RuntimeConnector::Podman(podman),
        }
    }

    pub fn connect(&self) -> RuntimeConnector {
        self.connector.clone()
    }
}
