use crate::build;
use crate::container::{DockerContainer, ExecOpts, Output};
use crate::docker::{api::ContainerCreateOpts, ExecContainerOpts};
use crate::image::ImageState;
use crate::ssh;
use crate::{Error, Result};

use std::path::Path;
use tracing::{info_span, trace, Instrument};

pub struct Context<'job> {
    pub container: DockerContainer<'job>,
    pub opts: ContainerCreateOpts,
    pub build: &'job build::Context,
}

impl<'job> Context<'job> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(build: &'job build::Context, opts: ContainerCreateOpts) -> Context<'job> {
        Context {
            container: DockerContainer::new(&build.docker, Some(build.is_running.clone())),
            opts,
            build,
        }
    }
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
            .env(env.kv_vec())
            .working_dir(ctx.container_bld_dir.to_string_lossy())
            .build();

        let mut ctx = Context::new(&ctx, opts);
        ctx.container.spawn(&ctx.opts).await.map(|_| ctx)
    }
    .instrument(span)
    .await
}

pub async fn checked_exec(ctx: &Context<'_>, opts: &ExecContainerOpts) -> Result<Output<String>> {
    let span = info_span!("checked-exec");
    async move {
        let out = ctx.container.exec(opts, ctx.build.quiet).await?;
        if out.exit_code != 0 {
            Err(Error::msg(format!(
                "command failed with exit code {}\nError:\n{}",
                out.exit_code,
                out.stderr.join("\n")
            )))
        } else {
            Ok(out)
        }
    }
    .instrument(span)
    .await
}

pub async fn create_dirs<P: AsRef<Path>>(ctx: &Context<'_>, dirs: &[P]) -> Result<()> {
    let span = info_span!("create-dirs");
    async move {
        let dirs_joined =
            dirs.iter()
                .map(P::as_ref)
                .fold(String::new(), |mut dirs_joined, path| {
                    dirs_joined.push_str(&format!(" {}", path.display()));
                    dirs_joined
                });
        let dirs_joined = dirs_joined.trim();
        trace!(directories = %dirs_joined);

        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd(&format!("mkdir -pv {}", dirs_joined))
                .build(),
        )
        .await
        .map(|_| ())
    }
    .instrument(span)
    .await
}
