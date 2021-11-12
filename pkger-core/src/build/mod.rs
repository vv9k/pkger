#[macro_use]
pub mod container;
pub mod deps;
pub mod image;
pub mod package;
pub mod patches;
pub mod remote;
pub mod scripts;

use crate::container::ExecOpts;
use crate::docker::Docker;
use crate::gpg::GpgKey;
use crate::image::{Image, ImageState, ImagesState};
use crate::recipe::{ImageTarget, Recipe, RecipeTarget};
use crate::ssh::SshConfig;
use crate::{ErrContext, Result};

use async_rwlock::RwLock;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{info, info_span, trace, warn, Instrument};

#[derive(Debug)]
/// Groups all data and functionality necessary to create an artifact
pub struct Context {
    id: String,
    recipe: Arc<Recipe>,
    image: Image,
    docker: Docker,
    container_bld_dir: PathBuf,
    container_out_dir: PathBuf,
    container_tmp_dir: PathBuf,
    out_dir: PathBuf,
    target: RecipeTarget,
    image_state: Arc<RwLock<ImagesState>>,
    simple: bool,
    gpg_key: Option<GpgKey>,
    ssh: Option<SshConfig>,
    quiet: bool,
}

impl Context {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        recipe: Arc<Recipe>,
        image: Image,
        docker: Docker,
        target: ImageTarget,
        out_dir: &Path,
        image_state: Arc<RwLock<ImagesState>>,
        simple: bool,
        gpg_key: Option<GpgKey>,
        ssh: Option<SshConfig>,
        quiet: bool,
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
        trace!(id = %id, "creating new build context");

        let target = RecipeTarget::new(recipe.metadata.name.clone(), target);

        Context {
            id,
            recipe,
            image,
            docker,
            container_bld_dir,
            container_out_dir,
            container_tmp_dir,
            out_dir: out_dir.to_path_buf(),
            target,
            image_state,
            simple,
            gpg_key,
            ssh,
            quiet,
        }
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    async fn create_out_dir(&self, image: &ImageState) -> Result<PathBuf> {
        let span = info_span!("create-out-dir");
        async move {
            let out_dir = self.out_dir.join(&image.image);

            if out_dir.exists() {
                trace!(dir = %out_dir.display(), "already exists, skipping");
                Ok(out_dir)
            } else {
                trace!(dir = %out_dir.display(), "creating directory");
                fs::create_dir_all(out_dir.as_path())
                    .map(|_| out_dir)
                    .context("failed to create output directory")
            }
        }
        .instrument(span)
        .await
    }
}

pub async fn run(ctx: &mut Context) -> Result<PathBuf> {
    let span = info_span!("build", recipe = %ctx.recipe.metadata.name, image = %ctx.target.image(), target = %ctx.target.build_target().as_ref());
    async move {
        info!(id = %ctx.id, "running job" );
        let image_state = image::build(ctx).await.context("failed to build image")?;

        let out_dir = ctx.create_out_dir(&image_state).await?;

        let mut container_ctx = container::spawn(ctx, &image_state).await?;

        let image_state = if image_state.tag != image::CACHED {
            let mut deps = deps::default(
                ctx.target.build_target(),
                &ctx.recipe,
                ctx.gpg_key.is_some(),
            );
            deps.extend(deps::recipe(&container_ctx, &image_state));
            let new_state =
                image::create_cache(&container_ctx, &ctx.docker, &image_state, &deps).await?;
            info!(id = %new_state.id, image = %new_state.image, "successfully cached image");

            trace!("saving image state");
            let mut state = ctx.image_state.write().await;
            (*state).update(ctx.target.clone(), new_state.clone());

            container_ctx.container.remove().await?;
            container_ctx = container::spawn(ctx, &new_state).await?;

            new_state
        } else {
            image_state
        };

        let dirs = vec![
            &ctx.container_out_dir,
            &ctx.container_bld_dir,
            &ctx.container_tmp_dir,
        ];

        container::create_dirs(&container_ctx, &dirs[..]).await?;

        remote::fetch_source(&container_ctx).await?;

        if let Some(patches) = &ctx.recipe.metadata.patches {
            let patches = patches::collect(&container_ctx, patches).await?;
            patches::apply(&container_ctx, patches).await?;
        }

        scripts::run(&container_ctx).await?;

        exclude_paths(&container_ctx).await?;

        let package = package::build(&container_ctx, &image_state, out_dir.as_path()).await?;

        container_ctx.container.remove().await?;

        Ok(package)
    }
    .instrument(span)
    .await
}

pub async fn exclude_paths(ctx: &container::Context<'_>) -> Result<()> {
    let span = info_span!("exclude-paths");
    async move {
        if let Some(exclude) = &ctx.build.recipe.metadata.exclude {
            let exclude_paths = exclude
                .iter()
                .filter(|p| {
                    let p = PathBuf::from(p);
                    if p.is_absolute() {
                        warn!(path = %p.display(), "absolute paths are not allowed in excludes");
                        false
                    } else {
                        true
                    }
                })
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            info!(exclude_dirs = ?exclude_paths);

            container::checked_exec(
                ctx,
                &ExecOpts::default()
                    .cmd(&format!("rm -rvf {}", exclude_paths.join(" ")))
                    .working_dir(&ctx.build.container_out_dir)
                    .build(),
            )
            .await?;
        }

        Ok(())
    }
    .instrument(span)
    .await
}
