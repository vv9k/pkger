#[macro_use]
pub mod container;
pub mod deps;
pub mod image;
pub mod package;
pub mod patches;
pub mod remote;
pub mod scripts;

use crate::container::ExecOpts;
use crate::gpg::GpgKey;
use crate::image::{Image, ImageState, ImagesState};
use crate::log::{debug, info, trace, warning, write_out, BoxedCollector};
use crate::proxy::ProxyConfig;
use crate::recipe::{ImageTarget, Recipe, RecipeTarget};
use crate::runtime::RuntimeConnector;
use crate::ssh::SshConfig;
use crate::{ErrContext, Result};

use async_rwlock::RwLock;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use uuid::Uuid;

/// Groups all data and functionality necessary to create an artifact
pub struct Context {
    id: String,
    session_id: Uuid,
    recipe: Recipe,
    image: Image,
    runtime: RuntimeConnector,
    container_bld_dir: PathBuf,
    container_out_dir: PathBuf,
    container_tmp_dir: PathBuf,
    out_dir: PathBuf,
    target: RecipeTarget,
    image_state: Arc<RwLock<ImagesState>>,
    simple: bool,
    gpg_key: Option<GpgKey>,
    ssh: Option<SshConfig>,
    proxy: ProxyConfig,
}

impl Context {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: &Uuid,
        recipe: Recipe,
        image: Image,
        connector: RuntimeConnector,
        target: ImageTarget,
        out_dir: &Path,
        image_state: Arc<RwLock<ImagesState>>,
        simple: bool,
        gpg_key: Option<GpgKey>,
        ssh: Option<SshConfig>,
        proxy: ProxyConfig,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let id = format!(
            "pkger-{}-{}-{}",
            &recipe.metadata.name, &target.image, &timestamp,
        );
        let container_bld_dir = PathBuf::from(format!(
            "/tmp/{}-build-{}",
            &recipe.metadata.name, &timestamp,
        ));
        let container_out_dir =
            PathBuf::from(format!("/tmp/{}-out-{}", &recipe.metadata.name, &timestamp,));

        let container_tmp_dir =
            PathBuf::from(format!("/tmp/{}-tmp-{}", &recipe.metadata.name, &timestamp,));
        trace!("creating new build context {}", id);

        let target = RecipeTarget::new(recipe.metadata.name.clone(), target);

        Context {
            id,
            session_id: *session_id,
            recipe,
            image,
            runtime: connector,
            container_bld_dir,
            container_out_dir,
            container_tmp_dir,
            out_dir: out_dir.to_path_buf(),
            target,
            image_state,
            simple,
            gpg_key,
            ssh,
            proxy,
        }
    }

    pub fn is_docker(&self) -> bool {
        matches!(self.runtime, RuntimeConnector::Docker(_))
    }

    pub fn is_podman(&self) -> bool {
        matches!(self.runtime, RuntimeConnector::Podman(_))
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    async fn create_out_dir(
        &mut self,
        logger: &mut BoxedCollector,
        image: &ImageState,
    ) -> Result<PathBuf> {
        let out_dir = self.out_dir.join(&image.image);
        debug!(logger => "creating output directory `{}`", out_dir.display());

        if out_dir.exists() {
            trace!(logger => "directory already exists, skipping");
            Ok(out_dir)
        } else {
            trace!(logger => "creating directory");
            fs::create_dir_all(out_dir.as_path())
                .map(|_| out_dir)
                .context("failed to create output directory")
        }
    }
}

pub async fn run(ctx: &mut Context, logger: &mut BoxedCollector) -> Result<PathBuf> {
    info!(logger => "starting build, id = {}, recipe = {}, image = {}, target = {}", ctx.id, ctx.recipe.metadata.name, ctx.target.image(), ctx.target.build_target().as_ref());
    logger.append_scope(ctx.recipe.metadata.name.clone());
    logger.append_scope(ctx.target.image().to_string());
    let image_state = image::build(ctx, logger)
        .await
        .context("failed to build image")?;

    let out_dir = ctx.create_out_dir(logger, &image_state).await?;

    let mut container_ctx = container::spawn(ctx, &image_state, logger).await?;

    let image_state = if image_state.tag != image::CACHED {
        trace!(logger => "image tag is not {}, caching", image::CACHED);
        let mut deps = deps::default(
            ctx.target.build_target(),
            &ctx.recipe,
            ctx.gpg_key.is_some(),
        );
        trace!(logger => "default deps: {:?}", deps);
        let recipe_deps = deps::recipe(&container_ctx, &image_state);
        trace!(logger => "recipe deps: {:?}", recipe_deps);
        deps.extend(recipe_deps);
        let new_state = image::create_cache(&container_ctx, &image_state, &deps, logger).await?;

        info!(logger => "successfully cached image, id = {}, image = {}", &new_state.id, &new_state.image);

        info!(logger => "saving image state");
        let mut state = ctx.image_state.write().await;
        (*state).update(ctx.target.clone(), new_state.clone());

        container_ctx.container.remove(logger).await?;
        container_ctx = container::spawn(ctx, &new_state, logger).await?;

        new_state
    } else {
        image_state
    };

    let dirs = vec![
        &ctx.container_out_dir,
        &ctx.container_bld_dir,
        &ctx.container_tmp_dir,
    ];

    container_ctx.create_dirs(&dirs[..], logger).await?;

    remote::fetch_source(&container_ctx, logger).await?;

    if let Some(patches) = &ctx.recipe.metadata.patches {
        let patches = patches::collect(&container_ctx, patches, logger).await?;
        patches::apply(&container_ctx, patches, logger).await?;
    }

    scripts::run(&container_ctx, logger).await?;

    exclude_paths(&container_ctx, logger).await?;

    let package = package::build(&container_ctx, &image_state, out_dir.as_path(), logger).await?;

    container_ctx.container.remove(logger).await?;

    logger.pop_scope();
    logger.pop_scope();

    Ok(package)
}

pub async fn exclude_paths(
    ctx: &container::Context<'_>,
    logger: &mut BoxedCollector,
) -> Result<()> {
    info!(logger => "excluding paths");
    if let Some(exclude) = &ctx.build.recipe.metadata.exclude {
        let exclude_paths = exclude
            .iter()
            .filter(|p| {
                let p = PathBuf::from(p);
                if p.is_absolute() {
                    warning!(logger => "absolute paths are not allowed in excludes - '{}'", p.display());
                    false
                } else {
                    true
                }
            })
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        info!(logger => "exclude_dirs = {:?}", exclude_paths);

        ctx.checked_exec(
            &ExecOpts::default()
                .cmd(&format!("rm -rvf {}", exclude_paths.join(" ")))
                .working_dir(&ctx.build.container_out_dir),
            logger,
        )
        .await?;
    }

    Ok(())
}
