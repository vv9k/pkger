use crate::archive::{create_tarball, unpack_tarball};
use crate::{ErrContext, Result};

use docker_api::{
    api::{ContainerCreateOpts, ExecContainerOpts, LogsOpts, RmContainerOpts},
    conn::TtyChunk,
    Container, Docker, Exec,
};
use futures::{StreamExt, TryStreamExt};
use std::path::Path;
use std::str;
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

    pub fn build(self) -> ExecContainerOpts {
        let mut builder = ExecContainerOpts::builder();
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
            mut_builder = mut_builder.working_dir(working_dir.to_string_lossy());
        }

        if let Some(env) = self.env {
            mut_builder = mut_builder.env(env);
        }

        mut_builder.build()
    }
}

/// Wrapper type that allows easier manipulation of Docker containers
pub struct DockerContainer<'job> {
    container: Container<'job>,
    docker: &'job Docker,
}

impl<'job> DockerContainer<'job> {
    pub fn new(docker: &'job Docker) -> DockerContainer<'job> {
        Self {
            container: docker.containers().get(""),
            docker,
        }
    }

    pub fn inner(&self) -> &Container<'job> {
        &self.container
    }

    pub fn id(&self) -> &str {
        convert_id(self.container.id())
    }

    pub async fn spawn(&mut self, opts: &ContainerCreateOpts) -> Result<()> {
        let span = info_span!("container-spawn");
        async move {
            let container = self.docker.containers().create(opts).await?.id().to_owned();

            self.container = self.docker.containers().get(container);
            info!(id = %self.id(), "created container");

            self.container.start().await?;
            info!(id = %self.id(), "started container");

            Ok(())
        }
        .instrument(span)
        .await
    }

    pub async fn remove(&self) -> Result<()> {
        let span = info_span!("container-remove");
        async move {
            info!(id = %self.id(), "stopping container");
            self.container
                .kill(None)
                .await
                .context("failed to stop container")?;

            info!(id = %self.id(), "deleting container");
            self.container
                .remove(&RmContainerOpts::builder().force(true).build())
                .await
                .context("failed to delete container")?;

            Ok(())
        }
        .instrument(span)
        .await
    }

    pub async fn exec<'cmd>(
        &self,
        opts: &ExecContainerOpts,
        quiet: bool,
    ) -> Result<Output<String>> {
        let span = info_span!("container-exec", id = %self.id());
        async move {
            let exec = Exec::create(self.docker, self.id(), opts).await?;
            let mut stream = exec.start();

            let mut output = Output::default();

            while let Some(result) = stream.next().await {
                match result? {
                    TtyChunk::StdOut(chunk) => {
                        let chunk = str::from_utf8(&chunk)?;
                        output.stdout.push(chunk.to_string());
                        if !quiet {
                            chunk.lines().for_each(|line| {
                                info!("{}", line.trim());
                            })
                        }
                    }
                    TtyChunk::StdErr(chunk) => {
                        let chunk = str::from_utf8(&chunk)?;
                        output.stderr.push(chunk.to_string());
                        if !quiet {
                            chunk.lines().for_each(|line| {
                                error!("{}", line.trim());
                            })
                        }
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
                .logs(&LogsOpts::builder().stdout(stdout).stderr(stderr).build());

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

    pub async fn upload_files<'files, F, E, P>(
        &self,
        files: F,
        destination: P,
        quiet: bool,
    ) -> Result<()>
    where
        F: IntoIterator<Item = (E, &'files [u8])>,
        E: AsRef<Path>,
        P: AsRef<Path>,
    {
        let span = info_span!("upload-files");
        let cloned_span = span.clone();
        async move {
            let destination = destination.as_ref();
            let tar = cloned_span
                .in_scope(|| create_tarball(files.into_iter()))
                .context("failed creating a tarball with files")?;
            let tar_path = destination.join("archive.tgz");

            self.inner()
                .copy_file_into(&tar_path, &tar)
                .await
                .context("failed to copy archive with files to container")?;

            trace!("extract archive with files");
            self.exec(
                &ExecOpts::default()
                    .cmd(&format!("tar -xf {}", tar_path.display()))
                    .working_dir(destination)
                    .build(),
                quiet,
            )
            .await
            .map(|_| ())
            .context("failed to extract archive with with files")
        }
        .instrument(span)
        .await
    }
}
