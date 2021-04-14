use crate::job::{Ctx, JobCtx, container::convert_id};
use crate::Result;

use async_trait::async_trait;
use futures::StreamExt;
use moby::{tty::TtyChunk, ContainerOptions, Docker, LogsOptions};
use std::time::SystemTime;
use tracing::{trace,info_span, Instrument, info};

#[derive(Debug, Default)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Output {
    pub fn push_chunk(&mut self, chunk: TtyChunk) {
        match chunk {
            TtyChunk::StdErr(mut inner) => self.stderr.append(&mut inner),
            TtyChunk::StdOut(mut inner) => self.stdout.append(&mut inner),
            _ => unreachable!(),
        }
    }
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
        let span = info_span!("oneshot-ctx", id = %self.id);
        let _enter = span.enter();

        info!("creating container");
        let (id, handle) = self
            .docker
            .containers()
            .create(self.opts)
            .instrument(span.clone())
            .await
            .map(|info| {
                let handle = self.docker.containers().get(&info.id);
                (info.id, handle)
            })
            .map_err(|e| anyhow!("failed to create a container - {}", e))?;
        let id = convert_id(&id);
        trace!(container_id = %id);

        handle.start().instrument(span.clone()).await?;
        info!("started container");

        let mut logs_stream = handle.logs(
            &LogsOptions::builder()
                .stdout(self.stdout)
                .stderr(self.stderr)
                .build(),
        );

        info!("collecting output");
        let mut output = Output::default();
        while let Some(chunk) = logs_stream.next().await {
            output.push_chunk(chunk?);
        }

        Ok(output)
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
