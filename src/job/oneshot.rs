use crate::job::JobCtx;
use crate::Result;

use futures::StreamExt;
use moby::{tty::TtyChunk, ContainerOptions, Docker, LogsOptions};

pub struct OneShotCtx<'j> {
    docker: &'j Docker,
    opts: &'j ContainerOptions,
    stdout: bool,
    stderr: bool,
}

pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl<'j> OneShotCtx<'j> {
    pub fn new(docker: &'j Docker, opts: &'j ContainerOptions, stdout: bool, stderr: bool) -> Self {
        Self {
            docker,
            opts,
            stdout,
            stderr,
        }
    }

    pub async fn run(&mut self) -> Result<Output> {
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
impl<'j> From<OneShotCtx<'j>> for JobCtx<'j> {
    fn from(ctx: OneShotCtx<'j>) -> Self {
        JobCtx::OneShot(ctx)
    }
}
