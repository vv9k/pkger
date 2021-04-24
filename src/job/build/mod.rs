mod deb;
mod deps;
mod gzip;
mod image;
mod remote;
mod rpm;
mod scripts;

use crate::image::{FsImage, ImageState, ImagesState};
use crate::job::{Ctx, JobCtx};
use crate::recipe::{BuildTarget, ImageTarget, Recipe};
use crate::Config;
use crate::Result;
use crate::{
    container::{DockerContainer, Output},
    recipe::RecipeTarget,
};

use async_trait::async_trait;
use moby::{ContainerOptions, Docker};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use tracing::{info, info_span, trace, warn, Instrument};

macro_rules! cleanup {
    ($ctx:ident) => {
        if !$ctx.is_running().await? {
            return Err(anyhow!("job interrupted by ctrl-c signal"));
        }
    };
}

#[derive(Debug)]
/// Groups all data and functionality necessary to create an artifact
pub struct BuildCtx {
    id: String,
    recipe: Recipe,
    image: FsImage,
    docker: Docker,
    container_bld_dir: PathBuf,
    container_out_dir: PathBuf,
    container_tmp_dir: PathBuf,
    out_dir: PathBuf,
    build_target: BuildTarget,
    target: RecipeTarget,
    config: Arc<Config>,
    image_state: Arc<RwLock<ImagesState>>,
    is_running: Arc<AtomicBool>,
}

#[async_trait]
impl Ctx for BuildCtx {
    type JobResult = Result<PathBuf>;

    fn id(&self) -> &str {
        &self.id
    }

    async fn run(&mut self) -> Self::JobResult {
        let span = info_span!("build", recipe = %self.recipe.metadata.name, image = %self.image.name, target = %self.build_target.as_ref());
        async move {
            info!(id = %self.id, "running job" );
            let image_state = self
                .image_build()
                .await
                .map_err(|e| anyhow!("failed to build image - {}", e))?;

            let out_dir = self.create_out_dir(&image_state).await?;

            let mut container_ctx = self.container_spawn(&image_state).await?;

            cleanup!(container_ctx);

            let image_state = if image_state.tag != image::CACHED {
                let mut deps = deps::pkger_deps(&self.build_target, &self.recipe);
                deps.extend(container_ctx.recipe_deps(&image_state));
                let new_state = container_ctx
                    .cache_image(&self.docker, &image_state, &deps)
                    .await?;
                info!(id = %new_state.id, image = %new_state.image, "successfully cached image");

                if let Ok(mut state) = self.image_state.write() {
                    trace!("saving image state");
                    (*state).update(&self.target, &new_state)
                }

                container_ctx.container.remove().await?;
                container_ctx = self.container_spawn(&new_state).await?;

                new_state
            } else {
                image_state
            };

            cleanup!(container_ctx);

            let dirs = vec![
                self.container_out_dir.to_string_lossy().to_string(),
                self.container_bld_dir.to_string_lossy().to_string(),
                self.container_tmp_dir.to_string_lossy().to_string(),
            ];

            container_ctx.create_dirs(&dirs[..]).await?;

            cleanup!(container_ctx);

            container_ctx.fetch_source().await?;

            cleanup!(container_ctx);

            container_ctx.execute_scripts().await?;

            cleanup!(container_ctx);

            container_ctx.exclude_paths().await?;

            cleanup!(container_ctx);

            let package = container_ctx
                .create_package(&image_state, out_dir.as_path())
                .await?;

            cleanup!(container_ctx);

            let _bytes = container_ctx.archive_output_dir().await?;

            cleanup!(container_ctx);

            container_ctx.container.remove().await?;

            Ok(package)
        }
        .instrument(span)
        .await
    }
}

impl BuildCtx {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        recipe: Recipe,
        image: FsImage,
        docker: Docker,
        build_target: BuildTarget,
        config: Arc<Config>,
        image_state: Arc<RwLock<ImagesState>>,
        is_running: Arc<AtomicBool>,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let id = format!(
            "pkger-{}-{}-{}",
            &recipe.metadata.name, &image.name, &timestamp,
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

        let target = RecipeTarget::new(
            recipe.metadata.name.clone(),
            ImageTarget::new(&image.name, &build_target),
        );

        BuildCtx {
            id,
            recipe,
            image,
            docker,
            container_bld_dir,
            container_out_dir,
            container_tmp_dir,
            out_dir: PathBuf::from(&config.output_dir),
            build_target,
            target,
            config,
            image_state,
            is_running,
        }
    }

    /// Creates and starts a container from the given ImageState
    async fn container_spawn(&self, image_state: &ImageState) -> Result<BuildContainerCtx<'_>> {
        let span = info_span!("init-container-ctx");
        async move {
            trace!(image = ?image_state);

            let mut env = self.recipe.env.clone();
            env.insert("PKGER_BLD_DIR", self.container_bld_dir.to_string_lossy());
            env.insert("PKGER_OUT_DIR", self.container_out_dir.to_string_lossy());
            env.insert("PKGER_OS", image_state.os.name());
            env.insert("PKGER_OS_VERSION", image_state.os.version());
            trace!(env = ?env);

            let opts = ContainerOptions::builder(&image_state.id)
                .name(&self.id)
                .cmd(vec!["sleep infinity"])
                .entrypoint(vec!["/bin/sh", "-c"])
                .env(env.kv_vec())
                .working_dir(
                    self.container_bld_dir
                        .to_string_lossy()
                        .to_string()
                        .as_str(),
                )
                .build();

            let mut ctx = BuildContainerCtx::new(
                &self.docker,
                opts,
                &self.recipe,
                &self.image,
                self.is_running.clone(),
                self.build_target.clone(),
                self.container_out_dir.as_path(),
                self.container_bld_dir.as_path(),
                self.container_tmp_dir.as_path(),
            );

            ctx.start_container().await.map(|_| ctx)
        }
        .instrument(span)
        .await
    }

    async fn create_out_dir(&self, image: &ImageState) -> Result<PathBuf> {
        let span = info_span!("create-out-dir");
        async move {
            let os_ver = image.os.version();
            let out_dir = self.out_dir.join(format!("{}/{}", image.os.name(), os_ver));

            if out_dir.exists() {
                trace!(dir = %out_dir.display(), "already exists, skipping");
                Ok(out_dir)
            } else {
                trace!(dir = %out_dir.display(), "creating directory");
                fs::create_dir_all(out_dir.as_path())
                    .map(|_| out_dir)
                    .map_err(|e| anyhow!("failed to create output directory - {}", e))
            }
        }
        .instrument(span)
        .await
    }
}

impl<'job> From<BuildCtx> for JobCtx<'job> {
    fn from(ctx: BuildCtx) -> Self {
        JobCtx::Build(ctx)
    }
}

pub struct BuildContainerCtx<'job> {
    pub container: DockerContainer<'job>,
    pub opts: ContainerOptions,
    pub recipe: &'job Recipe,
    pub image: &'job FsImage,
    pub target: BuildTarget,
    pub container_out_dir: &'job Path,
    pub container_bld_dir: &'job Path,
    pub container_tmp_dir: &'job Path,
}

impl<'job> BuildContainerCtx<'job> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        docker: &'job Docker,
        opts: ContainerOptions,
        recipe: &'job Recipe,
        image: &'job FsImage,
        is_running: Arc<AtomicBool>,
        target: BuildTarget,
        container_out_dir: &'job Path,
        container_bld_dir: &'job Path,
        container_tmp_dir: &'job Path,
    ) -> BuildContainerCtx<'job> {
        BuildContainerCtx {
            container: DockerContainer::new(docker, Some(is_running)),
            opts,
            recipe,
            image,
            target,
            container_out_dir,
            container_bld_dir,
            container_tmp_dir,
        }
    }

    pub async fn is_running(&self) -> Result<bool> {
        self.container.is_running().await
    }

    pub async fn start_container(&mut self) -> Result<()> {
        self.container.spawn(&self.opts).await
    }

    pub async fn create_package(
        &self,
        image_state: &ImageState,
        output_dir: &Path,
    ) -> Result<PathBuf> {
        match self.target {
            BuildTarget::Rpm => self.build_rpm(&image_state, &output_dir).await,
            BuildTarget::Gzip => self.build_gzip(&output_dir).await,
            BuildTarget::Deb => self.build_deb(&image_state, &output_dir).await,
        }
    }

    pub async fn create_dirs<P: AsRef<Path>>(&self, dirs: &[P]) -> Result<()> {
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

            self.checked_exec(&format!("mkdir -pv {}", dirs_joined), None, None, None)
                .await
                .map(|_| ())
        }
        .instrument(span)
        .await
    }

    async fn checked_exec(
        &self,
        cmd: &str,
        working_dir: Option<&Path>,
        shell: Option<&str>,
        user: Option<&str>,
    ) -> Result<Output<String>> {
        let span = info_span!("checked-exec");
        async move {
            let out = self.container.exec(&cmd, working_dir, shell, user).await?;
            if out.exit_code != 0 {
                Err(anyhow!(
                    "command `{}` failed with exit code {}\nError:\n{}",
                    &cmd,
                    out.exit_code,
                    out.stderr.join("\n")
                ))
            } else {
                Ok(out)
            }
        }
        .instrument(span)
        .await
    }

    pub async fn exclude_paths(&self) -> Result<()> {
        let span = info_span!("exclude-paths");
        async move {
            if let Some(exclude) = &self.recipe.metadata.exclude {
                let exclude_paths = exclude
                .iter()
                .map(PathBuf::from)
                .filter(|p| {
                    if p.is_absolute() {
                        warn!(path = %p.display(), "absolute paths are not allowed in excludes");
                        false
                    } else {
                        true
                    }
                })
                .map(|p| p.to_string_lossy().to_string())
                .collect::<Vec<_>>();
                info!(exclude_dirs = ?exclude_paths);

                self.checked_exec(
                    &format!("rm -rvf {}", exclude_paths.join(" ")),
                    Some(self.container_out_dir),
                    None,
                    None,
                )
                .await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }
}
