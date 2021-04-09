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

impl<'j> OneShotCtx<'j> {
    pub fn new(docker: &'j Docker, opts: &'j ContainerOptions, stdout: bool, stderr: bool) -> Self {
        Self {
            docker,
            opts,
            stdout,
            stderr,
        }
    }
    pub async fn run(&mut self) -> Result<String> {
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
        let mut out = String::new();
        while let Some(chunk) = logs_stream.next().await {
            match chunk? {
                TtyChunk::StdOut(_chunk) => out.push_str(&String::from_utf8_lossy(&_chunk)),
                _ => {}
            }
        }

        Ok(out)
    }
}
impl<'j> From<OneShotCtx<'j>> for JobCtx<'j> {
    fn from(ctx: OneShotCtx<'j>) -> Self {
        JobCtx::OneShot(ctx)
    }
}
