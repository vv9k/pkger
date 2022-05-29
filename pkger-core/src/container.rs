use crate::archive::{create_tarball, unpack_tarball};
use crate::log::{debug, error, info, trace, BoxedCollector};
use crate::{ErrContext, Result};

use docker_api::{
    api::{
        ContainerCreateOpts, ContainerPruneFilter, ContainerPruneOpts, ContainersPruneInfo,
        ExecContainerOpts, LogsOpts, RmContainerOpts,
    },
    conn::TtyChunk,
    Container, Docker, Exec,
};
use futures::{StreamExt, TryStreamExt};
use std::path::Path;
use std::str;

/// Length of significant characters of a container ID.
static CONTAINER_ID_LEN: usize = 12;
static DEFAULT_SHELL: &str = "/bin/sh";

fn truncate(id: &str) -> &str {
    if id.len() > CONTAINER_ID_LEN {
        &id[..CONTAINER_ID_LEN]
    } else {
        id
    }
}

/// Removes invalid characters from the given name.
///
/// According to the error message allowed characters are [a-zA-Z0-9_.-].
pub fn fix_name(name: &str) -> String {
    name.chars()
        .filter(|&c| c.is_alphanumeric() || c == '-' || c == '.' || c == '_')
        .collect()
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

        trace!("{:?}", self);

        builder = builder
            .cmd(vec![self.shell, "-c", self.cmd])
            .tty(self.allocate_tty)
            .attach_stdout(self.attach_stdout)
            .attach_stderr(self.attach_stderr)
            .privileged(self.privileged);

        if let Some(user) = self.user {
            builder = builder.user(user);
        }

        if let Some(working_dir) = self.working_dir {
            builder = builder.working_dir(working_dir.to_string_lossy());
        }

        if let Some(env) = self.env {
            builder = builder.env(env);
        }

        builder.build()
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
        truncate(self.container.id())
    }

    pub async fn spawn(
        &mut self,
        opts: &ContainerCreateOpts,
        logger: &mut BoxedCollector,
    ) -> Result<()> {
        let container = self.docker.containers().create(opts).await?.id().to_owned();
        info!(logger => "spawning container {}", self.id());
        self.container = self.docker.containers().get(container);
        info!(logger => "created container {}", self.id());

        self.container.start().await?;
        info!(logger => "started container {}", self.id());

        Ok(())
    }

    pub async fn remove(&self, logger: &mut BoxedCollector) -> Result<()> {
        info!(logger => "removing container {}", self.id());
        info!(logger => "stopping container {}", self.id());
        self.container
            .kill(None)
            .await
            .context("failed to stop container")?;

        info!(logger => "deleting container {}", self.id());
        self.container
            .remove(&RmContainerOpts::builder().force(true).build())
            .await
            .context("failed to delete container")?;

        Ok(())
    }

    pub async fn exec<'cmd>(
        &self,
        opts: &ExecContainerOpts,
        logger: &mut BoxedCollector,
    ) -> Result<Output<String>> {
        debug!(logger => "executing command in container {}, {:?}", self.id(), opts);
        let exec = Exec::create(self.docker, self.id(), opts).await?;
        let mut stream = exec.start();

        let mut container_output = Output::default();

        while let Some(result) = stream.next().await {
            match result? {
                TtyChunk::StdOut(chunk) => {
                    let chunk = str::from_utf8(&chunk)?;
                    container_output.stdout.push(chunk.to_string());
                    chunk.lines().for_each(|line| {
                        info!(logger => "{}", line.trim());
                    })
                }
                TtyChunk::StdErr(chunk) => {
                    let chunk = str::from_utf8(&chunk)?;
                    container_output.stderr.push(chunk.to_string());
                    chunk.lines().for_each(|line| {
                        error!(logger => "{}", line.trim());
                    })
                }
                _ => unreachable!(),
            }
        }

        container_output.exit_code = exec
            .inspect()
            .await
            .map(|details| details.exit_code.unwrap_or_default())?;

        Ok(container_output)
    }

    pub async fn logs(
        &self,
        stdout: bool,
        stderr: bool,
        logger: &mut BoxedCollector,
    ) -> Result<Output<u8>> {
        debug!(logger => "collecting container logs for {}", self.id());
        trace!(logger => "stdout: {}, stderr: {}", stdout, stderr);

        let mut logs_stream = self
            .container
            .logs(&LogsOpts::builder().stdout(stdout).stderr(stderr).build());

        let mut output = Output::default();
        while let Some(chunk) = logs_stream.next().await {
            output.stdout.append(&mut chunk?.to_vec());
        }

        Ok(output)
    }

    pub async fn copy_from(&self, path: &Path, logger: &mut BoxedCollector) -> Result<Vec<u8>> {
        debug!(logger => "copying files from container {}, path: {}", self.id(), path.display());
        self.inner()
            .copy_from(path)
            .try_concat()
            .await
            .context("failed to copy from container")
    }

    pub async fn download_files(
        &self,
        source: &Path,
        dest: &Path,
        logger: &mut BoxedCollector,
    ) -> Result<()> {
        info!(logger => "downloading files from container {}, source: {}, destination: {}", self.id(), source.display(), dest.display());
        let files = self.copy_from(source, logger).await?;

        let mut archive = tar::Archive::new(&files[..]);

        unpack_tarball(&mut archive, dest, logger)
    }

    pub async fn upload_files<'files, F, E, P>(
        &self,
        files: F,
        destination: P,
        logger: &mut BoxedCollector,
    ) -> Result<()>
    where
        F: IntoIterator<Item = (E, &'files [u8])>,
        E: AsRef<Path>,
        P: AsRef<Path>,
    {
        let destination = destination.as_ref();
        let tar = create_tarball(files.into_iter(), logger)
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
            logger,
        )
        .await
        .map(|_| ())
        .context("failed to extract archive with with files")
    }
}

pub async fn cleanup(
    docker: &'_ Docker,
    key: impl Into<String>,
    value: impl Into<String>,
) -> Result<ContainersPruneInfo> {
    docker
        .containers()
        .prune(
            &ContainerPruneOpts::builder()
                .filter([ContainerPruneFilter::Label(key.into(), value.into())])
                .build(),
        )
        .await
        .context("cleaning up containers")
}
