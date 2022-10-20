use crate::archive::{create_tarball, unpack_tarball};
use crate::log::{debug, error, info, trace, BoxedCollector};
use crate::runtime::container::{truncate, Container, CreateOpts, ExecOpts, Output};
use crate::{unix_timestamp, ErrContext, Result};

use async_trait::async_trait;
use docker_api::{
    conn::TtyChunk,
    models::ContainerPrune200Response,
    opts::{ContainerPruneFilter, ContainerPruneOpts, ContainerRemoveOpts, LogsOpts},
    Docker, Exec,
};
use futures::{StreamExt, TryStreamExt};

use std::path::{Path, PathBuf};
use std::str;

#[cfg(unix)]
pub static DOCKER_SOCK: &str = "unix:///run/docker.sock";
#[cfg(not(unix))]
pub static DOCKER_SOCK: &str = "tcp://127.0.0.1:8080";
#[cfg(unix)]
pub static DOCKER_SOCK_SECONDARY: &str = "unix:///var/run/docker.sock";
#[cfg(not(unix))]
pub static DOCKER_SOCK_SECONDARY: &str = DOCKER_SOCK;

/// Wrapper type that allows easier manipulation of Docker containers
pub struct DockerContainer {
    container: docker_api::Container,
    docker: Docker,
}

impl DockerContainer {
    pub fn new(docker: Docker) -> DockerContainer {
        Self {
            container: docker.containers().get(""),
            docker,
        }
    }

    pub fn inner(&self) -> &docker_api::Container {
        &self.container
    }
}

#[async_trait]
impl Container for DockerContainer {
    fn id(&self) -> &str {
        truncate(self.container.id().as_ref())
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
            .remove(&ContainerRemoveOpts::builder().force(true).build())
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
        let exec =
            Exec::create(self.docker.clone(), self.id(), &opts.clone().build_docker()).await?;
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
            .map(|details| details.exit_code.unwrap_or_default() as u64)?;

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

    async fn upload_files<'files>(
        &self,
        files: Vec<(&Path, &'files [u8])>,
        destination: &Path,
        logger: &mut BoxedCollector,
    ) -> Result<()> {
        let tar = create_tarball(files.into_iter(), logger)
            .context("failed creating a tarball with files")?;

        self.upload_and_extract_archive(
            tar,
            destination,
            &format!("archive-{}", unix_timestamp().as_secs()),
            logger,
        )
        .await
    }

    async fn upload_archive(
        &self,
        tarball: Vec<u8>,
        destination: &Path,
        archive_name: &str,
        logger: &mut BoxedCollector,
    ) -> Result<PathBuf> {
        trace!(logger => "upload archive");
        let tar_path = destination.join(archive_name);

        self.inner()
            .copy_file_into(&tar_path, &tarball)
            .await
            .map(|_| tar_path)
            .context("failed to copy archive with files to container")
    }

    async fn upload_and_extract_archive(
        &self,
        tarball: Vec<u8>,
        destination: &Path,
        archive_name: &str,
        logger: &mut BoxedCollector,
    ) -> Result<()> {
        let tar_path = self
            .upload_archive(tarball, destination, archive_name, logger)
            .await?;
        trace!("extract archive with files");

        self.exec(
            &ExecOpts::default()
                .cmd(&format!("tar -xvf {0} && rm -f {0}", tar_path.display()))
                .working_dir(destination),
            logger,
        )
        .await
        .map(|_| ())
        .context("failed to extract archive with files to container")
    }
}

pub async fn cleanup(
    docker: &'_ Docker,
    key: impl Into<String>,
    value: impl Into<String>,
) -> Result<ContainerPrune200Response> {
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
