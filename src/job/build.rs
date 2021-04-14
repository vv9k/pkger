use crate::cleanup;
use crate::image::{Image, ImageState, ImagesState};
use crate::job::{container::DockerContainer, Ctx, JobCtx};
use crate::recipe::{BuildTarget, Recipe};
use crate::Config;
use crate::Result;

use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};
use moby::{image::ImageBuildChunk, BuildOptions, ContainerOptions, Docker};
use std::path::Path;
use std::path::PathBuf;
use std::str;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use tracing::{debug, info, info_span, trace, Instrument};

#[derive(Debug)]
/// Groups all data and functionality necessary to create an artifact
pub struct BuildCtx {
    id: String,
    recipe: Recipe,
    image: Image,
    docker: Docker,
    bld_dir: PathBuf,
    out_dir: PathBuf,
    target: BuildTarget,
    config: Arc<Config>,
    image_state: Arc<RwLock<ImagesState>>,
    is_running: Arc<AtomicBool>,
}

#[async_trait]
impl Ctx for BuildCtx {
    type JobResult = Result<()>;

    fn id(&self) -> &str {
        &self.id
    }

    async fn run(&mut self) -> Self::JobResult {
        let span =
            info_span!("build", recipe = %self.recipe.metadata.name, image = %self.image.name);
        let _enter = span.enter();

        info!(id = %self.id, "running job" );
        let image_state = self
            .image_build()
            .instrument(span.clone())
            .await
            .map_err(|e| anyhow!("failed to build image - {}", e))?;

        info!(image = %image_state.image);

        let container_ctx = self
            .container_spawn(&image_state)
            .instrument(span.clone())
            .await?;

        cleanup!(container_ctx, span);

        container_ctx
            .install_pkger_deps(&image_state)
            .instrument(span.clone())
            .await?;

        cleanup!(container_ctx, span);

        container_ctx
            .install_recipe_deps(&image_state)
            .instrument(span.clone())
            .await?;

        cleanup!(container_ctx, span);

        container_ctx.create_dirs().instrument(span.clone()).await?;

        cleanup!(container_ctx, span);

        container_ctx
            .execute_scripts()
            .instrument(span.clone())
            .await?;

        cleanup!(container_ctx, span);

        let _bytes = container_ctx
            .archive_output_dir()
            .instrument(span.clone())
            .await?;

        container_ctx.container.remove().await?;

        Ok(())
    }
}

impl BuildCtx {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        recipe: Recipe,
        image: Image,
        docker: Docker,
        target: BuildTarget,
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
        let bld_dir = PathBuf::from(format!(
            "/tmp/{}-build-{}",
            &recipe.metadata.name, &timestamp,
        ));
        let out_dir = PathBuf::from(format!("/tmp/{}-out-{}", &recipe.metadata.name, &timestamp,));
        trace!(id = %id, "creating new build context");

        BuildCtx {
            id,
            recipe,
            image,
            docker,
            bld_dir,
            out_dir,
            target,
            config,
            image_state,
            is_running,
        }
    }

    /// Creates and starts a container from the given ImageState
    async fn container_spawn(&self, image_state: &ImageState) -> Result<BuildContainerCtx<'_>> {
        let span = info_span!("container-spawn");
        let _enter = span.enter();

        let mut env = self.recipe.env.clone();
        env.insert("PKGER_BLD_DIR", self.bld_dir.to_string_lossy());
        env.insert("PKGER_OUT_DIR", self.out_dir.to_string_lossy());
        env.insert("PKGER_OS", image_state.os.as_ref());
        env.insert("PKGER_OS_VERSION", image_state.os.os_ver());
        trace!(env = ?env);

        let opts = ContainerOptions::builder(&image_state.image)
            .name(&self.id)
            .cmd(vec!["sleep infinity"])
            .entrypoint(vec!["/bin/sh", "-c"])
            .env(env.kv_vec())
            .working_dir(self.bld_dir.to_string_lossy().to_string().as_str())
            .build();

        let mut ctx = BuildContainerCtx::new(
            &self.docker,
            opts,
            &self.recipe,
            &self.image,
            self.is_running.clone(),
            self.target.clone(),
            self.bld_dir.as_path(),
            self.out_dir.as_path(),
        );

        ctx.start_container()
            .instrument(span.clone())
            .await
            .map(|_| ctx)
    }

    async fn image_build(&mut self) -> Result<ImageState> {
        let span = info_span!("image-build");
        let _enter = span.enter();

        if let Some(state) = self.image.find_cached_state(&self.image_state) {
            debug!(state = ?state, "found cached image state");
            return Ok(state);
        }

        debug!(image = %self.image.name, "building from scratch");
        let images = self.docker.images();
        let opts = BuildOptions::builder(self.image.path.to_string_lossy().to_string())
            .tag(&format!("{}:latest", &self.image.name))
            .build();

        let mut stream = images.build(&opts);

        while let Some(chunk) = stream.next().instrument(span.clone()).await {
            let chunk = chunk?;
            match chunk {
                ImageBuildChunk::Error {
                    error,
                    error_detail: _,
                } => {
                    return Err(anyhow!(error));
                }
                ImageBuildChunk::Update { stream } => {
                    info!("{}", stream);
                }
                ImageBuildChunk::Digest { aux } => {
                    let state = ImageState::new(
                        &aux.id,
                        &self.image.name,
                        "latest",
                        &SystemTime::now(),
                        &self.docker,
                    )
                    .instrument(span.clone())
                    .await?;

                    if let Ok(mut image_state) = self.image_state.write() {
                        (*image_state).update(&self.image.name, &state)
                    }

                    return Ok(state);
                }
                _ => {}
            }
        }

        Err(anyhow!("stream ended before image id was received"))
    }
}

impl<'job> From<BuildCtx> for JobCtx<'job> {
    fn from(ctx: BuildCtx) -> Self {
        JobCtx::Build(ctx)
    }
}

pub struct BuildContainerCtx<'job> {
    pub container: DockerContainer<'job>,
    opts: ContainerOptions,
    recipe: &'job Recipe,
    image: &'job Image,
    bld_dir: PathBuf,
    out_dir: PathBuf,
    target: BuildTarget,
}

impl<'job> BuildContainerCtx<'job> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        docker: &'job Docker,
        opts: ContainerOptions,
        recipe: &'job Recipe,
        image: &'job Image,
        is_running: Arc<AtomicBool>,
        target: BuildTarget,
        bld_dir: &Path,
        out_dir: &Path,
    ) -> BuildContainerCtx<'job> {
        BuildContainerCtx {
            container: DockerContainer::new(docker, Some(is_running)),
            opts,
            recipe,
            image,
            bld_dir: bld_dir.to_path_buf(),
            out_dir: out_dir.to_path_buf(),
            target,
        }
    }

    pub async fn check_is_running(&self) -> Result<bool> {
        self.container.check_is_running().await
    }

    pub async fn start_container(&mut self) -> Result<()> {
        self.container.spawn(&self.opts).await
    }

    pub async fn install_recipe_deps(&self, state: &ImageState) -> Result<()> {
        let deps = if let Some(deps) = &self.recipe.metadata.build_depends {
            deps.resolve_names(&state.image)
        } else {
            vec![]
        };

        self._install_deps(&deps, &state).await
    }

    pub async fn install_pkger_deps(&self, state: &ImageState) -> Result<()> {
        let mut deps = vec!["tar", "git"];
        match self.target {
            BuildTarget::Rpm => {
                deps.push("rpmbuild");
            }
            BuildTarget::Deb => {
                deps.push("dpkg-deb");
            }
            BuildTarget::Gzip => {
                deps.push("gzip");
            }
        }

        let deps = deps.into_iter().map(str::to_string).collect::<Vec<_>>();

        self._install_deps(&deps, &state).await
    }

    pub async fn execute_scripts(&self) -> Result<()> {
        let span = info_span!("exec-scripts", container = %self.container.id());
        let _enter = span.enter();

        if let Some(config_script) = &self.recipe.configure_script {
            info!("executing config scripts");
            for cmd in &config_script.steps {
                trace!(command = %cmd.cmd, "processing");
                if !cmd.images.is_empty() {
                    trace!(images = ?cmd.images, "only execute on");
                    if !cmd.images.contains(&self.image.name) {
                        trace!(image = %self.image.name, "not found, skipping");
                        continue;
                    }
                }
                trace!(command = %cmd.cmd, "running");
                self.container
                    .exec(&cmd.cmd)
                    .instrument(span.clone())
                    .await?;
            }
        }

        info!("executing build scripts");
        for cmd in &self.recipe.build_script.steps {
            trace!(command = %cmd.cmd, "processing");
            if !cmd.images.is_empty() {
                trace!(images = ?cmd.images, "only execute on");
                if !cmd.images.contains(&self.image.name) {
                    trace!(image = %self.image.name, "not found, skipping");
                    continue;
                }
            }
            trace!(command = %cmd.cmd, "running");
            self.container
                .exec(&cmd.cmd)
                .instrument(span.clone())
                .await?;
        }

        if let Some(install_script) = &self.recipe.install_script {
            info!("executing install scripts");
            for cmd in &install_script.steps {
                trace!(command = %cmd.cmd, "processing");
                if !cmd.images.is_empty() {
                    trace!(images = ?cmd.images, "only execute on");
                    if !cmd.images.contains(&self.image.name) {
                        trace!(image = %self.image.name, "not found, skipping");
                        continue;
                    }
                }
                trace!(command = %cmd.cmd, "running");
                self.container
                    .exec(&cmd.cmd)
                    .instrument(span.clone())
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn create_dirs(&self) -> Result<()> {
        let span = info_span!("create-dirs", container = %self.container.id());
        let _enter = span.enter();

        info!("creating necessary directories");
        let dirs = vec![
            self.out_dir.to_string_lossy().to_string(),
            self.bld_dir.to_string_lossy().to_string(),
        ]
        .join(" ");
        trace!(directories = %dirs);

        self.container
            .exec(format!("mkdir -pv {}", dirs))
            .instrument(span.clone())
            .await
    }

    pub async fn archive_output_dir(&self) -> Result<Vec<u8>> {
        let span = info_span!("archive-output", container = %self.container.id());

        info!("copying final archive");
        self.container
            .inner()
            .copy_from(self.out_dir.as_path())
            .try_concat()
            .instrument(span.clone())
            .await
            .map_err(|e| anyhow!("failed to archive output directory - {}", e))
    }

    async fn _install_deps(&self, deps: &[String], state: &ImageState) -> Result<()> {
        let span = info_span!("install-deps", container = %self.container.id());
        let _enter = span.enter();

        info!("installing dependencies");
        let pkg_mngr = state.os.package_manager();

        if deps.is_empty() {
            trace!("no dependencies to install");
            return Ok(());
        }

        let deps = deps.join(" ");

        trace!(deps = %deps, "resolved dependency names");

        let cmd = format!(
            "{} {} {}",
            pkg_mngr.as_ref(),
            pkg_mngr.install_args().join(" "),
            deps
        );
        trace!(command = %cmd, "installing with");

        self.container.exec(cmd).instrument(span.clone()).await
    }
}
