use crate::archive::{create_tarball, unpack_tarball};
use crate::container::{truncate, Container, CreateOpts, ExecOpts, Output};
use crate::log::{debug, error, info, trace, BoxedCollector};
use crate::{ErrContext, Result};

use async_trait::async_trait;
use docker_api::{
    api::{
        ContainerPruneFilter, ContainerPruneOpts, ContainersPruneInfo, LogsOpts, RmContainerOpts,
    },
    conn::TtyChunk,
    Docker, Exec,
};
use futures::{StreamExt, TryStreamExt};
use std::path::Path;
use std::str;

/// Wrapper type that allows easier manipulation of Docker containers
pub struct DockerContainer<'job> {
    container: docker_api::Container<'job>,
    docker: &'job Docker,
}

impl<'job> DockerContainer<'job> {
    pub fn new(docker: &'job Docker) -> DockerContainer<'job> {
        Self {
            container: docker.containers().get(""),
            docker,
        }
    }

    pub fn inner(&self) -> &docker_api::Container<'job> {
        &self.container
    }
}

#[async_trait]
impl<'job> Container<'job> for DockerContainer<'job> {
    type T = Docker;

    fn new(docker: &'job Self::T) -> DockerContainer<'job> {
        Self {
            container: docker.containers().get(""),
            docker,
        }
    }

    fn id(&self) -> &str {
        truncate(self.container.id())
    }

    async fn spawn(&mut self, opts: &CreateOpts, logger: &mut BoxedCollector) -> Result<()> {
        let container = self
            .docker
            .containers()
            .create(&opts.clone().build_docker())
            .await?
            .id()
            .to_owned();
        info!(logger => "spawning container {}", self.id());
        self.container = self.docker.containers().get(container);
        info!(logger => "created container {}", self.id());

        self.container.start().await?;
        info!(logger => "started container {}", self.id());

        Ok(())
    }

    async fn remove(&self, logger: &mut BoxedCollector) -> Result<()> {
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

    async fn exec<'cmd>(
        &self,
        opts: &ExecOpts,
        logger: &mut BoxedCollector,
    ) -> Result<Output<String>> {
        debug!(logger => "executing command in container {}, {:?}", self.id(), opts);
        let exec = Exec::create(self.docker, self.id(), &opts.clone().build_docker()).await?;
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

    async fn logs(
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

    async fn copy_from(&self, path: &Path, logger: &mut BoxedCollector) -> Result<Vec<u8>> {
        debug!(logger => "copying files from container {}, path: {}", self.id(), path.display());
        self.inner()
            .copy_from(path)
            .try_concat()
            .await
            .context("failed to copy from container")
    }

    async fn download_files(
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

    async fn upload_files<'files, F, E, P>(
        &self,
        files: F,
        destination: P,
        logger: &mut BoxedCollector,
    ) -> Result<()>
    where
        F: IntoIterator<Item = (E, &'files [u8])> + Send,
        E: AsRef<Path>,
        P: AsRef<Path> + Send,
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
                .working_dir(destination),
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
