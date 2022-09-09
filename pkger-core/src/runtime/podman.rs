use crate::archive::{create_tarball, unpack_tarball};
use crate::container::{truncate, Container, CreateOpts, ExecOpts, Output};
use crate::log::{debug, error, info, trace, BoxedCollector};
use crate::{ErrContext, Result};

use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};
use podman_api::{
    conn::TtyChunk,
    models::ContainersPruneReportLibpod,
    opts::{ContainerLogsOpts, ContainerPruneFilter, ContainerPruneOpts},
    Podman,
};
use std::path::{Path, PathBuf};
use std::str;

#[cfg(unix)]
pub static PODMAN_SOCK: &str = "unix:///run/user/1000/podman/podman.sock";
#[cfg(not(unix))]
pub static PODMAN_SOCK: &str = "tcp://127.0.0.1:8080";

/// Wrapper type that allows easier manipulation of Podman containers
pub struct PodmanContainer {
    container: podman_api::api::Container,
    podman: Podman,
}

impl PodmanContainer {
    pub fn new(podman: Podman) -> PodmanContainer {
        Self {
            container: podman.containers().get(""),
            podman,
        }
    }

    pub fn inner(&self) -> &podman_api::api::Container {
        &self.container
    }
}

#[async_trait]
impl Container for PodmanContainer {
    fn id(&self) -> &str {
        truncate(self.container.id().as_ref())
    }

    async fn spawn(&mut self, opts: &CreateOpts, logger: &mut BoxedCollector) -> Result<()> {
        let container = self
            .podman
            .containers()
            .create(&opts.clone().build_podman())
            .await?
            .id;

        info!(logger => "spawning container {}", self.id());
        self.container = self.podman.containers().get(container);
        info!(logger => "created container {}", self.id());

        self.container.start(None).await?;
        info!(logger => "started container {}", self.id());

        Ok(())
    }

    async fn remove(&self, logger: &mut BoxedCollector) -> Result<()> {
        info!(logger => "removing container {}", self.id());
        info!(logger => "stopping container {}", self.id());
        self.container
            .kill()
            .await
            .context("failed to stop container")?;

        info!(logger => "deleting container {}", self.id());
        self.container
            .remove()
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
        let exec = self
            .inner()
            .create_exec(&opts.clone().build_podman())
            .await?;

        let opts = Default::default();
        let mut stream = exec.start(&opts);

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

        container_output.exit_code = exec.inspect().await.map(|details| {
            details
                .get("ExitCode")
                .and_then(|code| code.as_u64())
                .unwrap_or_default()
        })?;

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

        let mut logs_stream = self.container.logs(
            &ContainerLogsOpts::builder()
                .stdout(stdout)
                .stderr(stderr)
                .build(),
        );

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
                .cmd(&format!("tar -xf {0} && rm -f {0}", tar_path.display()))
                .working_dir(destination),
            logger,
        )
        .await
        .map(|_| ())
        .context("failed to extract archive with files to container")
    }
}

pub async fn cleanup(
    docker: &'_ Podman,
    key: impl Into<String>,
    value: impl Into<String>,
) -> Result<Vec<ContainersPruneReportLibpod>> {
    docker
        .containers()
        .prune(
            &ContainerPruneOpts::builder()
                .filter([ContainerPruneFilter::LabelKeyVal(key.into(), value.into())])
                .build(),
        )
        .await
        .context("cleaning up containers")
}
