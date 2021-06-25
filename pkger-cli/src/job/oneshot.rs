use crate::job::{Ctx, JobCtx};
use crate::Result;
use pkger_core::container::{DockerContainer, Output};

use async_trait::async_trait;
use moby::{ContainerOptions, Docker};
use std::time::SystemTime;
use tracing::{info_span, Instrument};

#[derive(Debug)]
/// Simple job that spawns a container with a command to execute and returns its stdout and/or
/// stderr.
pub struct OneShotCtx<'job> {
    id: String,
    docker: &'job Docker,
    opts: &'job ContainerOptions,
    stdout: bool,
    stderr: bool,
}

#[async_trait]
impl<'job> Ctx for OneShotCtx<'job> {
    type JobResult = Result<Output<u8>>;

    fn id(&self) -> &str {
        &self.id
    }

    async fn run(&mut self) -> Self::JobResult {
        let span = info_span!("oneshot-ctx", id = %self.id);
        async move {
            let mut container = DockerContainer::new(&self.docker, None);
            container.spawn(&self.opts).await?;

            container.logs(self.stdout, self.stderr).await
        }
        .instrument(span)
        .await
    }
}

impl<'job> OneShotCtx<'job> {
    pub fn new(
        docker: &'job Docker,
        opts: &'job ContainerOptions,
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
}
impl<'job> From<OneShotCtx<'job>> for JobCtx<'job> {
    fn from(ctx: OneShotCtx<'job>) -> Self {
        JobCtx::OneShot(ctx)
    }
}
