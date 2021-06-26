use crate::build;
use crate::container::{DockerContainer, ExecOpts, Output};
use crate::docker::{ContainerOptions, Docker, ExecContainerOptions};
use crate::image::ImageState;
use crate::recipe::{Recipe, RecipeTarget};
use crate::{Error, Result};

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tracing::{info_span, trace, Instrument};

#[allow(clippy::needless_lifetimes)] // it actually doesn't compile without them?
/// Creates and starts a container from the given ImageState
pub async fn spawn<'ctx>(
    ctx: &'ctx build::Context,
    image_state: &ImageState,
) -> Result<Context<'ctx>> {
    let span = info_span!("init-container-ctx");
    async move {
        trace!(image = ?image_state);

        let mut env = ctx.recipe.env.clone();
        env.insert("PKGER_BLD_DIR", ctx.container_bld_dir.to_string_lossy());
        env.insert("PKGER_OUT_DIR", ctx.container_out_dir.to_string_lossy());
        env.insert("PKGER_OS", image_state.os.name());
        env.insert("PKGER_OS_VERSION", image_state.os.version());
        trace!(env = ?env);

        let opts = ContainerOptions::builder(&image_state.id)
            .name(&ctx.id)
            .cmd(vec!["sleep infinity"])
            .entrypoint(vec!["/bin/sh", "-c"])
            .env(env.kv_vec())
            .working_dir(ctx.container_bld_dir.to_string_lossy().to_string().as_str())
            .build();

        let mut ctx = Context::new(
            &ctx.docker,
            opts,
            &ctx.recipe,
            ctx.target.image(),
            ctx.is_running.clone(),
            &ctx.target,
            ctx.container_out_dir.as_path(),
            ctx.container_bld_dir.as_path(),
            ctx.container_tmp_dir.as_path(),
            ctx.simple,
        );

        ctx.start_container().await.map(|_| ctx)
    }
    .instrument(span)
    .await
}

pub struct Context<'job> {
    pub container: DockerContainer<'job>,
    pub opts: ContainerOptions,
    pub recipe: &'job Recipe,
    pub image: &'job str,
    pub target: &'job RecipeTarget,
    pub container_out_dir: &'job Path,
    pub container_bld_dir: &'job Path,
    pub container_tmp_dir: &'job Path,
    pub simple: bool,
}

impl<'job> Context<'job> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        docker: &'job Docker,
        opts: ContainerOptions,
        recipe: &'job Recipe,
        image: &'job str,
        is_running: Arc<AtomicBool>,
        target: &'job RecipeTarget,
        container_out_dir: &'job Path,
        container_bld_dir: &'job Path,
        container_tmp_dir: &'job Path,
        simple: bool,
    ) -> Context<'job> {
        Context {
            container: DockerContainer::new(docker, Some(is_running)),
            opts,
            recipe,
            image,
            target,
            container_out_dir,
            container_bld_dir,
            container_tmp_dir,
            simple,
        }
    }

    pub async fn is_running(&self) -> Result<bool> {
        self.container.is_running().await
    }

    pub async fn start_container(&mut self) -> Result<()> {
        self.container.spawn(&self.opts).await
    }
}

pub async fn checked_exec(
    ctx: &Context<'_>,
    opts: &ExecContainerOptions,
) -> Result<Output<String>> {
    let span = info_span!("checked-exec");
    async move {
        let out = ctx.container.exec(opts).await?;
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
