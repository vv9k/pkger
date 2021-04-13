use crate::job::{Ctx, JobCtx};
use crate::Result;

use async_trait::async_trait;
use futures::StreamExt;
use moby::{tty::TtyChunk, ContainerOptions, Docker, LogsOptions};
use std::time::SystemTime;

pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

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
    type JobResult = Result<Output>;

    fn id(&self) -> &str {
        &self.id
    }

    async fn run(&mut self) -> Self::JobResult {
        let handle = self
            .docker
            .containers()
            .create(self.opts)
            .await
            .map(|info| self.docker.containers().get(info.id))
            .map_err(|e| anyhow!("failed to create a container - {}", e))?;

        handle.start().await?;

        let mut logs_stream = handle.logs(
            &LogsOptions::builder()
                .stdout(self.stdout)
                .stderr(self.stderr)
                .build(),
        );
        let mut stdout = vec![];
        let mut stderr = vec![];
        while let Some(chunk) = logs_stream.next().await {
            match chunk? {
                TtyChunk::StdOut(mut _chunk) => stdout.append(&mut _chunk),
                TtyChunk::StdErr(mut _chunk) => stderr.append(&mut _chunk),
                _ => {}
            }
        }

        Ok(Output { stdout, stderr })
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
