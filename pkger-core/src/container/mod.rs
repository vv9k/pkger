pub mod docker;

pub use docker::DockerContainer;

use crate::log::{trace, BoxedCollector};
use crate::Result;

use async_trait::async_trait;
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

#[derive(Clone, Default, Debug)]
pub struct CreateOpts {
    image: String,
    name: Option<String>,
    cmd: Option<Vec<String>>,
    entrypoint: Option<Vec<String>>,
    labels: Option<Vec<(String, String)>>,
    volumes: Option<Vec<String>>,
    env: Option<Vec<String>>,
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

    pub fn env(mut self, env: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.env = Some(env.into_iter().map(|e| e.into()).collect());
        self
    }

    pub fn working_dir(mut self, working_dir: impl Into<String>) -> Self {
        self.working_dir = Some(working_dir.into());
        self
    }

    pub fn build_docker(self) -> docker_api::api::ContainerCreateOpts {
        let mut builder = docker_api::api::ContainerCreateOpts::builder(self.image);

        if let Some(name) = self.name {
            builder = builder.name(name);
        }
        if let Some(cmd) = self.cmd {
            builder = builder.cmd(cmd);
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
            builder = builder.env(env);
        }
        if let Some(working_dir) = self.working_dir {
            builder = builder.working_dir(working_dir);
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

    pub fn build_docker(self) -> docker_api::ExecContainerOpts {
        let mut builder = docker_api::ExecContainerOpts::builder();

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

#[async_trait]
pub trait Container<'job> {
    type T;

    fn id(&self) -> &str;
    fn new(opts: &'job Self::T) -> Self;
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
    async fn upload_files<'files, F, EN, P>(
        &self,
        files: F,
        destination: P,
        logger: &mut BoxedCollector,
    ) -> Result<()>
    where
        F: IntoIterator<Item = (EN, &'files [u8])> + Send,
        EN: AsRef<Path>,
        P: AsRef<Path> + Send;
}
