use crate::Result;

pub use docker_api::*;

use std::path::PathBuf;

static RUN_DOCKER_SOCK: &str = "/run/docker.sock";
static VAR_RUN_DOCKER_SOCK: &str = "/var/run/docker.sock";

pub struct DockerConnectionPool {
    connector: Docker,
}

impl Default for DockerConnectionPool {
    fn default() -> Self {
        let socket_path = if PathBuf::from(RUN_DOCKER_SOCK).exists() {
            RUN_DOCKER_SOCK
        } else {
            VAR_RUN_DOCKER_SOCK
        };

        Self {
            connector: Docker::unix(socket_path),
        }
    }
}

impl DockerConnectionPool {
    pub fn new<S>(uri: S) -> Result<Self>
    where
        S: Into<String>,
    {
        let uri = uri.into();

        Ok(Self {
            connector: Docker::new(&uri)?,
        })
    }

    pub fn connect(&self) -> Docker {
        self.connector.clone()
    }
}
