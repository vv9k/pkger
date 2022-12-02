use crate::log::{trace, BoxedCollector};
use crate::recipe::Env;
use anyhow::{anyhow, Result};

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::str;

/// Length of significant characters of a container ID.
static CONTAINER_ID_LEN: usize = 12;
static DEFAULT_SHELL: &str = "/bin/sh";

pub(crate) fn truncate(id: &str) -> &str {
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

impl Output<String> {
    pub fn as_result(self) -> Result<Vec<String>> {
        if self.exit_code != 0 {
            Err(anyhow!(self.stderr.join("\n")))
        } else {
            Ok(self.stdout)
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct CreateOpts {
    image: String,
    name: Option<String>,
    cmd: Option<Vec<String>>,
    entrypoint: Option<Vec<String>>,
    labels: Option<Vec<(String, String)>>,
    volumes: Option<Vec<String>>,
    env: Option<Env>,
    working_dir: Option<String>,
}

impl CreateOpts {
    pub fn new(image: impl Into<String>) -> Self {
        CreateOpts {
            image: image.into(),
            ..Default::default()
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn cmd(mut self, command: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.cmd = Some(command.into_iter().map(|c| c.into()).collect());
        self
    }

    pub fn entrypoint(mut self, entrypoint: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.entrypoint = Some(entrypoint.into_iter().map(|e| e.into()).collect());
        self
    }

    pub fn labels(
        mut self,
        labels: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.labels = Some(
            labels
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        );
        self
    }

    pub fn volumes(mut self, volumes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.volumes = Some(volumes.into_iter().map(|v| v.into()).collect());
        self
    }

    pub fn env(mut self, env: Env) -> Self {
        self.env = Some(env);
        self
    }

    pub fn working_dir(mut self, working_dir: impl Into<String>) -> Self {
        self.working_dir = Some(working_dir.into());
        self
    }

    pub fn build_docker(self) -> docker_api::opts::ContainerCreateOpts {
        let mut builder = docker_api::opts::ContainerCreateOpts::builder().image(self.image);

        if let Some(name) = self.name {
            builder = builder.name(name);
        }
        if let Some(cmd) = self.cmd {
            builder = builder.command(cmd);
        }
        if let Some(entrypoint) = self.entrypoint {
            builder = builder.entrypoint(entrypoint);
        }
        if let Some(labels) = self.labels {
            builder = builder.labels(labels);
        }
        if let Some(volumes) = self.volumes {
            builder = builder.volumes(volumes);
        }
        if let Some(env) = self.env {
            builder = builder.env(env.kv_vec());
        }
        if let Some(working_dir) = self.working_dir {
            builder = builder.working_dir(working_dir);
        }

        builder.build()
    }

    pub fn build_podman(self) -> podman_api::opts::ContainerCreateOpts {
        let mut builder = podman_api::opts::ContainerCreateOpts::builder();

        builder = builder.image(self.image);

        if let Some(name) = self.name {
            builder = builder.name(name);
        }
        if let Some(cmd) = self.cmd {
            builder = builder.command(cmd);
        }
        if let Some(entrypoint) = self.entrypoint {
            builder = builder.entrypoint(entrypoint);
        }
        if let Some(labels) = self.labels {
            builder = builder.labels(labels);
        }
        if let Some(env) = self.env {
            builder = builder.env(env.iter());
        }
        if let Some(working_dir) = self.working_dir {
            builder = builder.work_dir(working_dir);
            builder = builder.create_working_dir(true);
        }

        builder.build()
    }
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
    env: Option<Env>,
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

    pub fn build_docker(self) -> docker_api::opts::ExecCreateOpts {
        let mut builder = docker_api::opts::ExecCreateOpts::builder();

        trace!("{:?}", self);

        builder = builder
            .command(vec![self.shell, "-c", self.cmd])
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
            builder = builder.env(env.kv_vec());
        }

        builder.build()
    }

    pub fn build_podman(self) -> podman_api::opts::ExecCreateOpts {
        use podman_api::opts::UserOpt;
        let mut builder = podman_api::opts::ExecCreateOpts::builder();

        trace!("{:?}", self);

        builder = builder
            .command(vec![self.shell, "-c", self.cmd])
            .tty(self.allocate_tty)
            .attach_stdout(self.attach_stdout)
            .attach_stderr(self.attach_stderr)
            .privileged(self.privileged);

        if let Some(user) = self.user {
            builder = builder.user(UserOpt::User(user.into()));
        }

        if let Some(working_dir) = self.working_dir {
            builder = builder.working_dir(working_dir.to_string_lossy());
        }

        if let Some(env) = self.env {
            builder = builder.env(env.iter());
        }

        builder.build()
    }
}

#[async_trait]
pub trait Container {
    fn id(&self) -> &str;
    async fn spawn(&mut self, opts: &CreateOpts, logger: &mut BoxedCollector) -> Result<()>;
    async fn remove(&self, logger: &mut BoxedCollector) -> Result<()>;
    async fn exec<'cmd>(
        &self,
        opts: &ExecOpts,
        logger: &mut BoxedCollector,
    ) -> Result<Output<String>>;
    async fn logs(
        &self,
        stdout: bool,
        stderr: bool,
        logger: &mut BoxedCollector,
    ) -> Result<Output<u8>>;
    async fn copy_from(&self, path: &Path, logger: &mut BoxedCollector) -> Result<Vec<u8>>;
    async fn download_files(
        &self,
        source: &Path,
        dest: &Path,
        logger: &mut BoxedCollector,
    ) -> Result<()>;
    async fn upload_files<'files>(
        &self,
        files: Vec<(&Path, &'files [u8])>,
        destination: &Path,
        logger: &mut BoxedCollector,
    ) -> Result<()>;
    async fn upload_archive(
        &self,
        tarball: Vec<u8>,
        destination: &Path,
        archive_name: &str,
        logger: &mut BoxedCollector,
    ) -> Result<PathBuf>;
    async fn upload_and_extract_archive(
        &self,
        tarball: Vec<u8>,
        destination: &Path,
        archive_name: &str,
        logger: &mut BoxedCollector,
    ) -> Result<()>;
}
