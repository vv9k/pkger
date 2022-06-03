use crate::container::{Container, CreateOpts, Output};
use crate::log::BoxedCollector;
use crate::runtime::{DockerContainer, PodmanContainer, RuntimeConnector};
use crate::Result;

use std::time::SystemTime;

#[derive(Debug)]
/// Simple job that spawns a container with a command to execute and returns its stdout and/or
/// stderr.
pub struct OneShotCtx<'job> {
    id: String,
    runtime: &'job RuntimeConnector,
    opts: &'job CreateOpts,
    stdout: bool,
    stderr: bool,
}

pub async fn run(ctx: &OneShotCtx<'_>, logger: &mut BoxedCollector) -> Result<Output<u8>> {
    match ctx.runtime {
        RuntimeConnector::Docker(docker) => {
            let mut container = DockerContainer::new(docker.clone());
            container.spawn(ctx.opts, logger).await?;

            container.logs(ctx.stdout, ctx.stderr, logger).await
        }
        RuntimeConnector::Podman(podman) => {
            let mut container = PodmanContainer::new(podman.clone());
            container.spawn(ctx.opts, logger).await?;

            container.logs(ctx.stdout, ctx.stderr, logger).await
        }
    }
}

impl<'job> OneShotCtx<'job> {
    pub fn new(
        runtime: &'job RuntimeConnector,
        opts: &'job CreateOpts,
        stdout: bool,
        stderr: bool,
    ) -> Self {
        let id = format!(
            "pkger-oneshot-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );

        Self {
            id,
            runtime,
            opts,
            stdout,
            stderr,
        }
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }
}
