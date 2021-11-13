use crate::build;
use crate::container::{DockerContainer, ExecOpts, Output};
use crate::docker::{api::ContainerCreateOpts, ExecContainerOpts};
use crate::image::ImageState;
use crate::ssh;
use crate::{err, ErrContext, Error, Result};

use crate::recipe::Env;
use std::path::Path;
use tracing::{info_span, trace, Instrument};

macro_rules! _exec {
    ($cmd: expr) => {
        ExecOpts::default().cmd($cmd)
    };
    ($cmd: expr, $working_dir: expr) => {
        _exec!($cmd).working_dir($working_dir)
    };
    ($cmd: expr, $working_dir: expr, $user: expr) => {
        _exec!($cmd).working_dir($working_dir).user($user)
    };
}

macro_rules! exec {
    ($cmd: expr) => {
        _exec!($cmd).build()
    };
    ($cmd: expr, $working_dir: expr) => {
        _exec!($cmd, $working_dir).build()
    };
    ($cmd: expr, $working_dir: expr, $user: expr) => {
        _exec!($cmd, $working_dir, $user).build()
    };
}

// https://github.com/rust-lang/rust-clippy/issues/7271
#[allow(clippy::needless_lifetimes)]
/// Creates and starts a container from the given ImageState
pub async fn spawn<'ctx>(
    ctx: &'ctx build::Context,
    image_state: &ImageState,
) -> Result<Context<'ctx>> {
    let span = info_span!("init-container-ctx");
    async move {
        trace!(image = ?image_state);

        let mut volumes = Vec::new();

        let mut env = ctx.recipe.env.clone();
        env.insert("PKGER_BLD_DIR", ctx.container_bld_dir.to_string_lossy());
        env.insert("PKGER_OUT_DIR", ctx.container_out_dir.to_string_lossy());
        env.insert("PKGER_OS", image_state.os.name());
        env.insert("PKGER_OS_VERSION", image_state.os.version());
        env.insert("RECIPE", &ctx.recipe.metadata.name);
        env.insert("RECIPE_VERSION", &ctx.recipe.metadata.version);
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

        trace!(env = ?env);

        let opts = ContainerCreateOpts::builder(&image_state.id)
            .name(&ctx.id)
            .cmd(vec!["sleep infinity"])
            .entrypoint(vec!["/bin/sh", "-c"])
            .volumes(volumes)
            .env(env.clone().kv_vec())
            .working_dir(ctx.container_bld_dir.to_string_lossy())
            .build();

        let mut ctx = Context::new(ctx, opts);
        ctx.set_env(env);
        ctx.container.spawn(&ctx.opts).await.map(|_| ctx)
    }
    .instrument(span)
    .await
}

pub struct Context<'job> {
    pub container: DockerContainer<'job>,
    pub opts: ContainerCreateOpts,
    pub build: &'job build::Context,
    pub vars: Env,
}

impl<'job> Context<'job> {
    pub fn new(build: &'job build::Context, opts: ContainerCreateOpts) -> Context<'job> {
        Context {
            container: DockerContainer::new(&build.docker),
            opts,
            build,
            vars: Env::new(),
        }
    }

    pub fn set_env(&mut self, env: Env) {
        self.vars = env;
    }

    pub async fn checked_exec(&self, opts: &ExecContainerOpts) -> Result<Output<String>> {
        let span = info_span!("checked-exec");
        async move {
            let out = self.container.exec(opts, self.build.quiet).await?;
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
        .instrument(span)
        .await
    }

    pub async fn script_exec(
        &self,
        script: impl IntoIterator<Item = (&ExecContainerOpts, Option<&'static str>)>,
    ) -> Result<()> {
        let span = info_span!("script-exec");
        async move {
            for (opts, context) in script.into_iter() {
                let mut res = self.checked_exec(opts).await.map(|_| ());
                if let Some(context) = context {
                    res = res.context(context);
                }
                if res.is_err() {
                    return res;
                }
            }
            Ok(())
        }
        .instrument(span)
        .await
    }

    pub async fn create_dirs<P: AsRef<Path>>(&self, dirs: &[P]) -> Result<()> {
        let span = info_span!("create-dirs");
        async move {
            let dirs_joined =
                dirs.iter()
                    .map(P::as_ref)
                    .fold(String::new(), |mut dirs_joined, path| {
                        dirs_joined.push(' ');
                        dirs_joined.push_str(&path.to_string_lossy());
                        dirs_joined
                    });
            let dirs_joined = dirs_joined.trim();
            trace!(directories = %dirs_joined);

            self.checked_exec(&exec!(&format!("mkdir -pw {}", dirs_joined)))
                .await
                .map(|_| ())
        }
        .instrument(span)
        .await
    }
}
