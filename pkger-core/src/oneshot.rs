use crate::container::{Container, CreateOpts, DockerContainer, Output};
use crate::docker::Docker;
use crate::log::BoxedCollector;
use crate::Result;

use std::time::SystemTime;

#[derive(Debug)]
/// Simple job that spawns a container with a command to execute and returns its stdout and/or
/// stderr.
pub struct OneShotCtx<'job> {
    id: String,
    docker: &'job Docker,
    opts: &'job CreateOpts,
    stdout: bool,
    stderr: bool,
}

pub async fn run(ctx: &OneShotCtx<'_>, logger: &mut BoxedCollector) -> Result<Output<u8>> {
    let mut container = DockerContainer::new(ctx.docker);
    container.spawn(ctx.opts, logger).await?;

    container.logs(ctx.stdout, ctx.stderr, logger).await
}

impl<'job> OneShotCtx<'job> {
    pub fn new(docker: &'job Docker, opts: &'job CreateOpts, stdout: bool, stderr: bool) -> Self {
        let id = format!(
            "pkger-oneshot-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );

        Self {
            id,
            docker,
            opts,
            stdout,
            stderr,
        }
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }
}
