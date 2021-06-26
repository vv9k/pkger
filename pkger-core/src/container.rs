use crate::archive::unpack_tarball;
use crate::{ErrContext, Result};

use futures::{StreamExt, TryStreamExt};
use moby::{
    tty::TtyChunk, Container, ContainerOptions, Docker, Exec, ExecContainerOptions, LogsOptions,
};
use std::path::Path;
use std::str;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{error, info, info_span, trace, Instrument};

/// Length of significant characters of a container ID.
static CONTAINER_ID_LEN: usize = 12;
static DEFAULT_SHELL: &str = "/bin/sh";

pub fn convert_id(id: &str) -> &str {
    &id[..CONTAINER_ID_LEN]
}

#[derive(Debug, Default)]
pub struct Output<T> {
    pub stdout: Vec<T>,
    pub stderr: Vec<T>,
    pub exit_code: u64,
}

#[derive(Clone, Debug)]
pub struct ExecOpts<'opts> {
    cmd: &'opts str,
    allocate_tty: bool,
    attach_stdout: bool,
    attach_stderr: bool,
    privileged: bool,
    shell: &'opts str,
    user: Option<&'opts str>,
    working_dir: Option<&'opts Path>,
    env: Option<&'opts [String]>,
}

impl<'opts> Default for ExecOpts<'opts> {
    fn default() -> Self {
        Self {
            cmd: "",
            allocate_tty: false,
            attach_stderr: true,
            attach_stdout: true,
            privileged: false,
            shell: DEFAULT_SHELL,
            user: None,
            working_dir: None,
            env: None,
        }
    }
}

#[allow(dead_code)]
impl<'opts> ExecOpts<'opts> {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

    pub fn cmd(mut self, command: &'opts str) -> Self {
        self.cmd = command;
        self
    }

    pub fn tty(mut self, allocate: bool) -> Self {
        self.allocate_tty = allocate;
        self
    }

    pub fn attach_stdout(mut self, attach: bool) -> Self {
        self.attach_stdout = attach;
        self
    }

    pub fn attach_stderr(mut self, attach: bool) -> Self {
        self.attach_stderr = attach;
        self
    }

    pub fn privileged(mut self, privileged: bool) -> Self {
        self.privileged = privileged;
        self
    }

    pub fn user(mut self, user: &'opts str) -> Self {
        self.user = Some(user);
        self
    }

    pub fn shell(mut self, shell: &'opts str) -> Self {
        self.shell = shell;
        self
    }

    pub fn working_dir(mut self, working_dir: &'opts Path) -> Self {
        self.working_dir = Some(working_dir);
        self
    }

    pub fn build(self) -> ExecContainerOptions {
        let mut builder = ExecContainerOptions::builder();
        let mut mut_builder = &mut builder;

        trace!(exec = ?self);

        mut_builder = mut_builder
            .cmd(vec![self.shell, "-c", self.cmd])
            .tty(self.allocate_tty)
            .attach_stdout(self.attach_stdout)
            .attach_stderr(self.attach_stderr)
            .privileged(self.privileged);

        if let Some(user) = self.user {
            mut_builder = mut_builder.user(user);
        }

        if let Some(working_dir) = self.working_dir {
            mut_builder = mut_builder.working_dir(working_dir.to_string_lossy().to_string());
        }

        if let Some(env) = self.env {
            mut_builder = mut_builder.env(env);
        }

        mut_builder.build()
    }
}

/// Wrapper type that allows easier manipulation of Docker containers
pub struct DockerContainer<'job> {
    /// Whether the main process is still running or got an exit signal
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
                .context("failed to stop container")?;

            info!("deleting container");
            self.container
                .delete()
                .await
                .context("failed to delete container")?;

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

    pub async fn exec<'cmd>(&self, opts: &ExecContainerOptions) -> Result<Output<String>> {
        let span = info_span!("container-exec", id = %self.id());
        async move {
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

    pub async fn copy_from(&self, path: &Path) -> Result<Vec<u8>> {
        let span = info_span!("copy-from", path = %path.display());
        async move {
            trace!("copying");
            self.inner()
                .copy_from(path)
                .try_concat()
                .await
                .context("failed to copy from container")
        }
        .instrument(span)
        .await
    }

    pub async fn download_files(&self, source: &Path, dest: &Path) -> Result<()> {
        let span = info_span!("container-download-files", id = %self.id(), source = %source.display(), destination = %dest.display());
        let cloned_span = span.clone();

        async move {
            trace!("fetching");
            let files = self.copy_from(source).await?;

            let mut archive = tar::Archive::new(&files[..]);

            cloned_span.in_scope(|| unpack_tarball(&mut archive, dest))
        }
        .instrument(span)
        .await
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
