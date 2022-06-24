use crate::build;
use crate::container::{fix_name, Container, CreateOpts, ExecOpts, Output};
use crate::image::ImageState;
use crate::log::{debug, info, trace, BoxedCollector};
use crate::runtime::{DockerContainer, PodmanContainer, RuntimeConnector};
use crate::ssh;
use crate::{err, ErrContext, Error, Result};

use crate::recipe::Env;
use std::path::Path;

pub static SESSION_LABEL_KEY: &str = "pkger.session";

// https://github.com/rust-lang/rust-clippy/issues/7271
#[allow(clippy::needless_lifetimes)]
/// Creates and starts a container from the given ImageState
pub async fn spawn<'ctx>(
    ctx: &'ctx build::Context,
    image_state: &ImageState,
    logger: &mut BoxedCollector,
) -> Result<Context<'ctx>> {
    info!(logger => "initializing container context");
    trace!(logger => "{:?}", image_state);

    if !ctx.recipe.metadata.version.has_version(&ctx.build_version) {
        return err!("invalid recipe version {}", ctx.build_version);
    }

    let mut volumes = Vec::new();

    let mut env = ctx.recipe.env.clone();
    env.insert("PKGER_BLD_DIR", ctx.container_bld_dir.to_string_lossy());
    env.insert("PKGER_OUT_DIR", ctx.container_out_dir.to_string_lossy());
    env.insert("PKGER_OS", image_state.os.name());
    env.insert("PKGER_OS_VERSION", image_state.os.version());
    env.insert("RECIPE", &ctx.recipe.metadata.name);
    env.insert("RECIPE_VERSION", &ctx.build_version);
    env.insert("RECIPE_RELEASE", ctx.recipe.metadata.release());

    if let Some(ssh) = &ctx.ssh {
        if ssh.forward_agent {
            const CONTAINER_PATH: &str = "/ssh-agent";
            let host_path = ssh::auth_sock()?;
            volumes.push(format!("{}:{}", host_path, CONTAINER_PATH));
            env.insert(ssh::SOCK_ENV, CONTAINER_PATH);
        }

        if ssh.disable_key_verification {
            env.insert("GIT_SSH_COMMAND", "ssh -o StrictHostKeyChecking=no");
        }
    }

    trace!("{:?}", env);

    let session_label = ctx.session_id.to_string();

    let opts = CreateOpts::new(&image_state.id)
        .name(&fix_name(&ctx.id))
        .cmd(["sleep infinity"])
        .entrypoint(["/bin/sh", "-c"])
        .labels([(SESSION_LABEL_KEY, session_label.as_str())])
        .volumes(volumes)
        .env(env.clone())
        .working_dir(ctx.container_bld_dir.to_string_lossy());

    let mut ctx = Context::new(ctx, opts);
    ctx.set_env(env);
    ctx.container.spawn(&ctx.opts, logger).await?;
    Ok(ctx)
}

pub struct Context<'job> {
    pub container: Box<dyn Container + Send + Sync>,
    pub opts: CreateOpts,
    pub build: &'job build::Context,
    pub vars: Env,
}

impl<'job> Context<'job> {
    pub fn new(build: &'job build::Context, opts: CreateOpts) -> Context<'_> {
        Context {
            container: match &build.runtime {
                RuntimeConnector::Docker(docker) => Box::new(DockerContainer::new(docker.clone())),
                RuntimeConnector::Podman(podman) => Box::new(PodmanContainer::new(podman.clone())),
            },
            opts,
            build,
            vars: Env::new(),
        }
    }

    pub fn set_env(&mut self, env: Env) {
        self.vars = env;
    }

    pub async fn checked_exec(
        &self,
        opts: &ExecOpts<'_>,
        logger: &mut BoxedCollector,
    ) -> Result<Output<String>> {
        debug!(logger => "running checked exec");
        let out = self.container.exec(opts, logger).await?;
        if out.exit_code != 0 {
            err!(
                "command failed with exit code {}\nError:\n{}",
                out.exit_code,
                out.stderr.join("\n")
            )
        } else {
            Ok(out)
        }
    }

    pub async fn script_exec(
        &self,
        script: impl IntoIterator<Item = (ExecOpts<'_>, Option<&'static str>)>,
        logger: &mut BoxedCollector,
    ) -> Result<()> {
        debug!(logger => "executing script");
        for (opts, context) in script.into_iter() {
            let mut res = self.checked_exec(&opts, logger).await.map(|_| ());
            if let Some(context) = context {
                res = res.context(context);
            }

            #[allow(clippy::question_mark)]
            if res.is_err() {
                return res;
            }
        }
        Ok(())
    }

    pub async fn create_dirs<P: AsRef<Path>>(
        &self,
        dirs: &[P],
        logger: &mut BoxedCollector,
    ) -> Result<()> {
        let dirs_joined =
            dirs.iter()
                .map(P::as_ref)
                .fold(String::new(), |mut dirs_joined, path| {
                    dirs_joined.push(' ');
                    dirs_joined.push_str(&path.to_string_lossy());
                    dirs_joined
                });
        let dirs_joined = dirs_joined.trim();
        info!(logger => "creating directories");
        debug!(logger => "Directories: {}", dirs_joined);

        self.checked_exec(
            &ExecOpts::new().cmd(&format!("mkdir -p {}", dirs_joined)),
            logger,
        )
        .await
        .map(|_| ())
    }
}
