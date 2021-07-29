use crate::container::{DockerContainer, Output};
use crate::docker::{api::ContainerCreateOpts, Docker};
use crate::Result;

use std::time::SystemTime;
use tracing::{info_span, Instrument};

#[derive(Debug)]
/// Simple job that spawns a container with a command to execute and returns its stdout and/or
/// stderr.
pub struct OneShotCtx<'job> {
    id: String,
    docker: &'job Docker,
    opts: &'job ContainerCreateOpts,
    stdout: bool,
    stderr: bool,
}

pub async fn run(ctx: &OneShotCtx<'_>) -> Result<Output<u8>> {
    let span = info_span!("oneshot-ctx", id = %ctx.id);
    async move {
        let mut container = DockerContainer::new(ctx.docker, None);
        container.spawn(ctx.opts).await?;

        container.logs(ctx.stdout, ctx.stderr).await
    }
    .instrument(span)
    .await
}

impl<'job> OneShotCtx<'job> {
    pub fn new(
        docker: &'job Docker,
        opts: &'job ContainerCreateOpts,
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
            docker,
            opts,
            stdout,
            stderr,
        }
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }
}
