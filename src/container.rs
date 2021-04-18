use crate::util::unpack_archive;
use crate::Result;

use futures::{StreamExt, TryStreamExt};
use moby::{
    tty::TtyChunk, Container, ContainerOptions, Docker, Exec, ExecContainerOptions, LogsOptions,
};
use std::path::Path;
use std::str;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, info_span, trace, Instrument};

/// Length of significant characters of a container ID.
const CONTAINER_ID_LEN: usize = 12;

pub fn convert_id(id: &str) -> &str {
    &id[..CONTAINER_ID_LEN]
}

#[derive(Debug, Default)]
pub struct Output<T> {
    pub stdout: Vec<T>,
    pub stderr: Vec<T>,
    pub exit_code: u64,
}

pub struct DockerContainer<'job> {
    is_running: Arc<AtomicBool>,
    container: Container<'job>,
    docker: &'job Docker,
}

impl<'job> DockerContainer<'job> {
    pub fn new(docker: &'job Docker, is_running: Option<Arc<AtomicBool>>) -> DockerContainer<'job> {
        Self {
            is_running: if let Some(is_running) = is_running {
                is_running
            } else {
                Arc::new(AtomicBool::new(true))
            },
            container: docker.containers().get(""),
            docker,
        }
    }

    pub fn inner(&self) -> &Container<'job> {
        &self.container
    }

    pub fn id(&self) -> &str {
        convert_id(&self.container.id())
    }

    pub async fn spawn(&mut self, opts: &ContainerOptions) -> Result<()> {
        let span = info_span!("container-spawn");
        async move {
            let id = self
                .docker
                .containers()
                .create(&opts)
                .await
                .map(|info| info.id)?;

            self.container = self.docker.containers().get(&id);
            info!(id = %self.id(), "created container");

            self.container.start().await?;
            info!(id = %self.id(), "started container");

            Ok(())
        }
        .instrument(span)
        .await
    }

    pub async fn remove(&self) -> Result<()> {
        let span = info_span!("container-remove", id = %self.id());
        async move {
            info!("stopping container");
            self.container
                .stop(None)
                .await
                .map_err(|e| anyhow!("failed to stop container - {}", e))?;

            info!("deleting container");
            self.container
                .delete()
                .await
                .map_err(|e| anyhow!("failed to delete container - {}", e))?;

            Ok(())
        }
        .instrument(span)
        .await
    }

    pub async fn is_running(&self) -> Result<bool> {
        let span = info_span!("is-running", id = %self.id());
        async move {
            if !self.is_running.load(Ordering::SeqCst) {
                trace!("not running");

                return self.remove().await.map(|_| false);
            }

            Ok(true)
        }
        .instrument(span)
        .await
    }

    pub async fn exec<S: AsRef<str>>(&self, cmd: S) -> Result<Output<String>> {
        let span = info_span!("container-exec", id = %self.id());
        async move {
            debug!(cmd = %cmd.as_ref(), "executing");

            let opts = ExecContainerOptions::builder()
                .cmd(vec!["/bin/sh", "-c", cmd.as_ref()])
                .attach_stdout(true)
                .attach_stderr(true)
                .build();

            let exec = Exec::create(&self.docker, self.id(), &opts).await?;
            let mut stream = exec.start();

            let mut output = Output::default();

            while let Some(result) = stream.next().await {
                self.check_ctrlc().await?;
                match result? {
                    TtyChunk::StdOut(chunk) => {
                        let chunk = str::from_utf8(&chunk)?;
                        output.stdout.push(chunk.to_string());
                        chunk.lines().for_each(|line| {
                            info!("{}", line.trim());
                        })
                    }
                    TtyChunk::StdErr(chunk) => {
                        let chunk = str::from_utf8(&chunk)?;
                        output.stderr.push(chunk.to_string());
                        chunk.lines().for_each(|line| {
                            error!("{}", line.trim());
                        })
                    }
                    _ => unreachable!(),
                }
            }

            output.exit_code = exec
                .inspect()
                .await
                .map(|details| details.exit_code.unwrap_or_default())?;

            Ok(output)
        }
        .instrument(span)
        .await
    }

    pub async fn logs(&self, stdout: bool, stderr: bool) -> Result<Output<u8>> {
        let span = info_span!("container-logs", id = %self.id());
        async move {
            trace!(stdout = %stdout, stderr = %stderr);

            let mut logs_stream = self
                .container
                .logs(&LogsOptions::builder().stdout(stdout).stderr(stderr).build());

            info!("collecting output");
            let mut output = Output::default();
            while let Some(chunk) = logs_stream.next().await {
                match chunk? {
                    TtyChunk::StdErr(mut inner) => output.stderr.append(&mut inner),
                    TtyChunk::StdOut(mut inner) => output.stdout.append(&mut inner),
                    _ => unreachable!(),
                }
            }

            Ok(output)
        }
        .instrument(span)
        .await
    }

    pub async fn download_files(&self, source: &Path, dest: &Path) -> Result<()> {
        let span = info_span!("container-download-files", id = %self.id(), source = %source.display(), destination = %dest.display());
        trace!(parent: &span, "fetching");

        let files = self
            .inner()
            .copy_from(source)
            .try_concat()
            .instrument(span.clone())
            .await?;

        let mut archive = tar::Archive::new(&files[..]);

        span.in_scope(|| {
            unpack_archive(&mut archive, dest)
                .map_err(|e| anyhow!("failed to unpack archive - {}", e))
        })
    }

    async fn check_ctrlc(&self) -> Result<()> {
        let span = info_span!("check-ctrlc");
        async move {
            if !self.is_running().await? {
                Err(anyhow!("container execution interrupted by ctrl-c signal"))
            } else {
                Ok(())
            }
        }
        .instrument(span)
        .await
    }
}
