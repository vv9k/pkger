#[macro_use]
pub mod container;
pub mod deps;
pub mod image;
pub mod package;
pub mod remote;
pub mod scripts;

use crate::container::ExecOpts;
use crate::docker::Docker;
use crate::gpg::GpgKey;
use crate::image::{Image, ImageState, ImagesState};
use crate::recipe::{ImageTarget, Patch, Patches, Recipe, RecipeTarget};
use crate::{ErrContext, Error, Result};

use async_rwlock::RwLock;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, info, info_span, trace, warn, Instrument};

macro_rules! cleanup {
    ($ctx:ident) => {
        if !$ctx.container.is_running().await? {
            return Err(Error::msg("job interrupted by ctrl-c signal"));
        }
    };
}

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
    is_running: Arc<AtomicBool>,
    simple: bool,
    gpg_key: Option<GpgKey>,
    forward_ssh_agent: bool,
}

pub async fn run(ctx: &mut Context) -> Result<PathBuf> {
    let span = info_span!("build", recipe = %ctx.recipe.metadata.name, image = %ctx.target.image(), target = %ctx.target.build_target().as_ref());
    async move {
        info!(id = %ctx.id, "running job" );
        let image_state = image::build(ctx).await.context("failed to build image")?;

        let out_dir = ctx.create_out_dir(&image_state).await?;

        let mut container_ctx = container::spawn(&ctx, &image_state).await?;

        cleanup!(container_ctx);

        let image_state = if image_state.tag != image::CACHED {
            let mut deps = deps::pkger_deps(
                ctx.target.build_target(),
                &ctx.recipe,
                ctx.gpg_key.is_some(),
            );
            deps.extend(deps::recipe_deps(&container_ctx, &image_state));
            let new_state =
                image::cache_image(&container_ctx, &ctx.docker, &image_state, &deps).await?;
            info!(id = %new_state.id, image = %new_state.image, "successfully cached image");

            trace!("saving image state");
            let mut state = ctx.image_state.write().await;
            (*state).update(&ctx.target, &new_state);

            container_ctx.container.remove().await?;
            container_ctx = container::spawn(&ctx, &new_state).await?;

            new_state
        } else {
            image_state
        };

        cleanup!(container_ctx);

        let dirs = vec![
            &ctx.container_out_dir,
            &ctx.container_bld_dir,
            &ctx.container_tmp_dir,
        ];

        container::create_dirs(&container_ctx, &dirs[..]).await?;

        cleanup!(container_ctx);

        remote::fetch_source(&container_ctx).await?;

        cleanup!(container_ctx);

        if let Some(patches) = &ctx.recipe.metadata.patches {
            let patches = collect_patches(&container_ctx, &patches).await?;

            cleanup!(container_ctx);

            apply_patches(&container_ctx, patches).await?;
        }

        cleanup!(container_ctx);

        scripts::execute_scripts(&container_ctx).await?;

        cleanup!(container_ctx);

        exclude_paths(&container_ctx).await?;

        cleanup!(container_ctx);

        let package =
            package::create_package(&container_ctx, &image_state, out_dir.as_path()).await?;

        container_ctx.container.remove().await?;

        Ok(package)
    }
    .instrument(span)
    .await
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
        is_running: Arc<AtomicBool>,
        simple: bool,
        gpg_key: Option<GpgKey>,
        forward_ssh_agent: bool,
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
            is_running,
            simple,
            gpg_key,
            forward_ssh_agent,
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
                &ctx,
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

pub async fn apply_patches(
    ctx: &container::Context<'_>,
    patches: Vec<(Patch, PathBuf)>,
) -> Result<()> {
    let span = info_span!("apply-patches");
    async move {
        trace!(patches = ?patches);
        for (patch, location) in patches {
            debug!(patch = ?patch, "applying");
            if let Err(e) = container::checked_exec(
                &ctx,
                &ExecOpts::default()
                    .cmd(&format!(
                        "patch -p{} < {}",
                        patch.strip_level(),
                        location.display()
                    ))
                    .working_dir(&ctx.build.container_bld_dir)
                    .build(),
            )
            .await
            {
                warn!(patch = ?patch, reason = %e, "applying failed");
            }
        }

        Ok(())
    }
    .instrument(span)
    .await
}

pub async fn collect_patches(
    ctx: &container::Context<'_>,
    patches: &Patches,
) -> Result<Vec<(Patch, PathBuf)>> {
    let span = info_span!("collect-patches");
    async move {
        let mut out = Vec::new();
        let patch_dir = ctx.build.container_tmp_dir.join("patches");
        container::create_dirs(&ctx, &[patch_dir.as_path()]).await?;

        let mut to_copy = Vec::new();

        for patch in patches.resolve_names(ctx.build.target.image()) {
            let src = patch.patch();
            if src.starts_with("http") {
                trace!(source = %src, "found http source");
                remote::get_http_source(ctx, src, &patch_dir).await?;
                out.push((
                    patch.clone(),
                    patch_dir.join(src.split('/').last().unwrap_or_default()),
                ));
                continue;
            }

            let patch_p = PathBuf::from(src);
            if patch_p.is_absolute() {
                trace!(path = %patch_p.display(), "found absolute path");
                out.push((
                    patch.clone(),
                    patch_dir.join(patch_p.file_name().unwrap_or_default()),
                ));
                to_copy.push(patch_p);
                continue;
            }

            let patch_recipe_p = ctx.build.recipe.recipe_dir.join(src);
            trace!(patch = %patch_recipe_p.display(), "using patch from recipe_dir");
            out.push((patch.clone(), patch_dir.join(src)));
            to_copy.push(patch_recipe_p);
        }

        let to_copy = to_copy.iter().map(PathBuf::as_path).collect::<Vec<_>>();

        let patches_archive = ctx.build.container_tmp_dir.join("patches.tar");
        remote::copy_files_into(ctx, &to_copy, &patches_archive).await?;

        container::checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd(&format!(
                    "tar xf {} -C {}",
                    patches_archive.display(),
                    patch_dir.display()
                ))
                .build(),
        )
        .await
        .map(|_| out)
    }
    .instrument(span)
    .await
}
