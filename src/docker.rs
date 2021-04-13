use crate::Result;

use moby::Docker;
pub struct DockerConnectionPool {
    connector: Docker,
}

impl Default for DockerConnectionPool {
    fn default() -> Self {
        Self {
            connector: Docker::unix("/run/docker.sock"),
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
