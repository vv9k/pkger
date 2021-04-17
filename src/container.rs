use crate::util::unpack_archive;
use crate::Result;

use futures::{StreamExt, TryStreamExt};
use moby::{tty::TtyChunk, Container, ContainerOptions, Docker, ExecContainerOptions, LogsOptions};
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
        let _enter = span.enter();

        let id = self
            .docker
            .containers()
            .create(&opts)
            .instrument(span.clone())
            .await
            .map(|info| info.id)?;
        self.container = self.docker.containers().get(&id);
        info!(parent: &span, id = %self.id(), "created container");

        self.container.start().instrument(span.clone()).await?;
        info!(parent: &span, id = %self.id(), "started container");

        Ok(())
    }

    pub async fn remove(&self) -> Result<()> {
        let span = info_span!("container-remove", id = %self.id());
        let _enter = span.enter();

        trace!(parent: &span, "stopping container");
        self.container
            .stop(None)
            .instrument(span.clone())
            .await
            .map_err(|e| anyhow!("failed to stop container - {}", e))?;

        trace!(parent: &span, "deleting container");
        self.container
            .delete()
            .instrument(span.clone())
            .await
            .map_err(|e| anyhow!("failed to delete container - {}", e))
    }

    pub async fn is_running(&self) -> Result<bool> {
        let span = info_span!("is-running", id = %self.id());
        let _enter = span.enter();

        if !self.is_running.load(Ordering::SeqCst) {
            trace!(parent: &span, "not running");

            return self.remove().instrument(span.clone()).await.map(|_| false);
        }

        Ok(true)
    }

    pub async fn exec<S: AsRef<str>>(&self, cmd: S) -> Result<Output<String>> {
        let span = info_span!("container-exec", id = %self.id());
        let _enter = span.enter();

        debug!(parent: &span, cmd = %cmd.as_ref(), "executing");

        let opts = ExecContainerOptions::builder()
            .cmd(vec!["/bin/sh", "-c", cmd.as_ref()])
            .attach_stdout(true)
            .attach_stderr(true)
            .build();

        let mut stream = self.container.exec(&opts);

        let mut output = Output::default();

        while let Some(result) = stream.next().instrument(span.clone()).await {
            self.check_ctrlc().instrument(span.clone()).await?;
            match result? {
                TtyChunk::StdOut(chunk) => {
                    let chunk = str::from_utf8(&chunk)?.trim_end_matches('\n');
                    output.stdout.push(chunk.to_string());
                    info!(parent: &span, "{}", chunk);
                }
                TtyChunk::StdErr(chunk) => {
                    let chunk = str::from_utf8(&chunk)?.trim_end_matches('\n');
                    output.stderr.push(chunk.to_string());
                    error!(parent: &span, "{}", chunk);
                }
                _ => unreachable!(),
            }
        }

        Ok(output)
    }

    pub async fn logs(&self, stdout: bool, stderr: bool) -> Result<Output<u8>> {
        let span = info_span!("container-logs", id = %self.id());
        let _enter = span.enter();

        trace!(parent: &span, stdout = %stdout, stderr = %stderr);

        let mut logs_stream = self
            .container
            .logs(&LogsOptions::builder().stdout(stdout).stderr(stderr).build());

        info!(parent: &span, "collecting output");
        let mut output = Output::default();
        while let Some(chunk) = logs_stream.next().instrument(span.clone()).await {
            match chunk? {
                TtyChunk::StdErr(mut inner) => output.stderr.append(&mut inner),
                TtyChunk::StdOut(mut inner) => output.stdout.append(&mut inner),
                _ => unreachable!(),
            }
        }

        Ok(output)
    }

    pub async fn download_files(&self, source: &Path, dest: &Path) -> Result<()> {
        let span =
            info_span!("download-files", source = %source.display(), destination = %dest.display());
        let _enter = span.enter();
        trace!("fetching");

        let files = self
            .inner()
            .copy_from(source)
            .try_concat()
            .instrument(span.clone())
            .await?;

        let mut archive = tar::Archive::new(&files[..]);

        async move {
            unpack_archive(&mut archive, dest)
                .map_err(|e| anyhow!("failed to unpack archive - {}", e))
        }
        .instrument(span.clone())
        .await
    }

    async fn check_ctrlc(&self) -> Result<()> {
        let span = info_span!("check-ctrlc");
        let _enter = span.enter();

        if !self.is_running().instrument(span.clone()).await? {
            Err(anyhow!("container execution interrupted by ctrl-c signal"))
        } else {
            Ok(())
        }
    }
}
