pub mod container;
pub mod docker;
pub mod podman;

pub use docker::DockerContainer;
pub use docker_api;
pub use podman::PodmanContainer;
pub use podman_api;

use crate::Result;

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
    pub fn docker<S>(uri: S) -> Result<Self>
    where
        S: Into<String>,
    {
        let uri = uri.into();

        Ok(Self {
            connector: RuntimeConnector::Docker(Docker::new(&uri)?),
        })
    }

    pub fn podman<S>(uri: S) -> Result<Self>
    where
        S: Into<String>,
    {
        let uri = uri.into();

        Ok(Self {
            connector: RuntimeConnector::Podman(Podman::new(&uri)?),
        })
    }

    pub fn connect(&self) -> RuntimeConnector {
        self.connector.clone()
    }
}
