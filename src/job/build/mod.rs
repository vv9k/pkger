mod deb;
mod rpm;

use crate::container::{DockerContainer, Output};
use crate::image::{Image, ImageState, ImagesState};
use crate::job::{Ctx, JobCtx};
use crate::recipe::{BuildTarget, Recipe};
use crate::util::save_tar_gz;
use crate::Config;
use crate::Result;

use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};
use moby::{image::ImageBuildChunk, BuildOptions, ContainerOptions, Docker};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::str;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use tracing::{debug, info, info_span, trace, Instrument};

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
    image: Image,
    docker: Docker,
    container_bld_dir: PathBuf,
    container_out_dir: PathBuf,
    out_dir: PathBuf,
    target: BuildTarget,
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
        let span = info_span!("build", recipe = %self.recipe.metadata.name, image = %self.image.name, target = %self.target.as_ref());
        async move {
            info!(id = %self.id, "running job" );
            let image_state = self
                .image_build()
                .await
                .map_err(|e| anyhow!("failed to build image - {}", e))?;

            let out_dir = self.create_out_dir(&image_state).await?;

            let container_ctx = self.container_spawn(&image_state).await?;

            cleanup!(container_ctx);

            let skip_deps = self.recipe.metadata.skip_default_deps.unwrap_or(false);

            if !skip_deps {
                container_ctx.install_pkger_deps(&image_state).await?;

                cleanup!(container_ctx);
            }

            container_ctx.install_recipe_deps(&image_state).await?;

            cleanup!(container_ctx);

            let dirs = vec![
                self.container_out_dir.to_string_lossy().to_string(),
                self.container_bld_dir.to_string_lossy().to_string(),
            ];

            container_ctx.create_dirs(&dirs[..]).await?;

            cleanup!(container_ctx);

            container_ctx.execute_scripts().await?;

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
        let container_bld_dir = PathBuf::from(format!(
            "/tmp/{}-build-{}",
            &recipe.metadata.name, &timestamp,
        ));
        let container_out_dir =
            PathBuf::from(format!("/tmp/{}-out-{}", &recipe.metadata.name, &timestamp,));
        trace!(id = %id, "creating new build context");

        BuildCtx {
            id,
            recipe,
            image,
            docker,
            container_bld_dir,
            container_out_dir,
            out_dir: PathBuf::from(&config.output_dir),
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
            env.insert("PKGER_OS", image_state.os.as_ref());
            env.insert("PKGER_OS_VERSION", image_state.os.os_ver());
            trace!(env = ?env);

            let opts = ContainerOptions::builder(&image_state.image)
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
                self.target.clone(),
                self.container_out_dir.as_path(),
            );

            ctx.start_container().await.map(|_| ctx)
        }
        .instrument(span)
        .await
    }

    async fn image_build(&mut self) -> Result<ImageState> {
        let span = info_span!("image-build");

        async move {
            if let Some(state) = self.image.find_cached_state(&self.image_state) {
                return Ok(state);
            }

            debug!(image = %self.image.name, "building from scratch");
            let images = self.docker.images();
            let opts = BuildOptions::builder(self.image.path.to_string_lossy().to_string())
                .tag(&format!("{}:latest", &self.image.name))
                .build();

            let mut stream = images.build(&opts);

            while let Some(chunk) = stream.next().await {
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
        .instrument(span)
        .await
    }

    async fn create_out_dir(&self, image: &ImageState) -> Result<PathBuf> {
        let span = info_span!("create-out-dir");
        async move {
            let os_ver = image.os.os_ver();
            let out_dir = self
                .out_dir
                .join(format!("{}/{}", image.os.as_ref(), os_ver));

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
    pub image: &'job Image,
    pub container_out_dir: &'job Path,
    pub target: BuildTarget,
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
        container_out_dir: &'job Path,
    ) -> BuildContainerCtx<'job> {
        BuildContainerCtx {
            container: DockerContainer::new(docker, Some(is_running)),
            opts,
            recipe,
            image,
            container_out_dir,
            target,
        }
    }

    pub async fn is_running(&self) -> Result<bool> {
        self.container.is_running().await
    }

    pub async fn start_container(&mut self) -> Result<()> {
        self.container.spawn(&self.opts).await
    }

    pub async fn install_recipe_deps(&self, state: &ImageState) -> Result<()> {
        let span = info_span!("recipe-deps");
        async move {
            let deps = if let Some(deps) = &self.recipe.metadata.build_depends {
                deps.resolve_names(&state.image)
            } else {
                vec![]
            };

            self._install_deps(&deps, &state).await
        }
        .instrument(span)
        .await
    }

    pub async fn install_pkger_deps(&self, state: &ImageState) -> Result<()> {
        let span = info_span!("default-deps");
        async move {
            let mut deps = vec!["tar", "git"];
            match self.target {
                BuildTarget::Rpm => {
                    deps.push("rpm-build");
                }
                BuildTarget::Deb => {
                    deps.push("dpkg");
                }
                BuildTarget::Gzip => {
                    deps.push("gzip");
                }
            }

            let deps = deps.into_iter().map(str::to_string).collect::<Vec<_>>();

            self._install_deps(&deps, &state).await
        }
        .instrument(span)
        .await
    }

    pub async fn execute_scripts(&self) -> Result<()> {
        let span = info_span!("exec-scripts");
        async move {
            if let Some(config_script) = &self.recipe.configure_script {
                let script_span = info_span!("configure");
                info!(parent: &script_span, "executing configure scripts");
                for cmd in &config_script.steps {
                    if !cmd.images.is_empty() {
                        trace!(parent: &script_span, images = ?cmd.images, "only execute on");
                        if !cmd.images.contains(&self.image.name) {
                            trace!(parent: &script_span, image = %self.image.name, "not found, skipping");
                            continue;
                        }
                    }
                    let out = self.checked_exec(&cmd.cmd).instrument(script_span.clone()).await?;

                    if out.exit_code != 0 {
                        return Err(anyhow!(
                            "command `{}` failed with exit code {}\nError:\n{}",
                            &cmd.cmd,
                            out.exit_code,
                            out.stderr.join("\n")
                        ));
                    }
                }
            } else {
                info!("no configure steps to run");
            }

            let script_span = info_span!("build");
            info!(parent: &script_span, "executing build scripts");
            for cmd in &self.recipe.build_script.steps {
                if !cmd.images.is_empty() {
                    trace!(parent: &script_span, images = ?cmd.images, "only execute on");
                    if !cmd.images.contains(&self.image.name) {
                        trace!(parent: &script_span, image = %self.image.name, "not found, skipping");
                        continue;
                    }
                }

                self.checked_exec(&cmd.cmd).instrument(script_span.clone()).await?;
            }

            if let Some(install_script) = &self.recipe.install_script {
                let script_span = info_span!("install");
                info!(parent: &script_span, "executing install scripts");
                for cmd in &install_script.steps {
                    if !cmd.images.is_empty() {
                        trace!(parent: &script_span, images = ?cmd.images, "only execute on");
                        if !cmd.images.contains(&self.image.name) {
                            trace!(parent: &script_span, image = %self.image.name, "not found, skipping");
                            continue;
                        }
                    }

                    self.checked_exec(&cmd.cmd).instrument(script_span.clone()).await?;
                }
            } else {
                info!("no install steps to run");
            }

            Ok(())
        }
        .instrument(span)
        .await
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

            self.checked_exec(&format!("mkdir -pv {}", dirs_joined))
                .await
                .map(|_| ())
        }
        .instrument(span)
        .await
    }

    pub async fn archive_output_dir(&self) -> Result<Vec<u8>> {
        let span = info_span!("archive-output", container_dir = %self.container_out_dir.display());
        async move {
            info!("copying final archive");
            self.container
                .inner()
                .copy_from(self.container_out_dir)
                .try_concat()
                .await
                .map_err(|e| anyhow!("failed to archive output directory - {}", e))
        }
        .instrument(span)
        .await
    }

    /// Creates a final GZIP package and saves it to `output_dir` returning the path of the final
    /// archive as String.
    async fn build_gzip(&self, output_dir: &Path) -> Result<PathBuf> {
        let span = info_span!("GZIP");
        info!(parent: &span, "building GZIP package");
        let package = self
            .container
            .inner()
            .copy_from(self.container_out_dir)
            .try_concat()
            .instrument(span.clone())
            .await?;

        let archive = tar::Archive::new(&package[..]);
        let archive_name = format!(
            "{}-{}.tar.gz",
            &self.recipe.metadata.name, &self.recipe.metadata.version
        );

        span.in_scope(|| {
            save_tar_gz(archive, &archive_name, output_dir)
                .map_err(|e| anyhow!("failed to save package as tar.gz - {}", e))
        })
        .map(|_| output_dir.join(archive_name))
    }

    async fn checked_exec(&self, cmd: &str) -> Result<Output<String>> {
        let span = info_span!("checked-exec");
        async move {
            let out = self.container.exec(&cmd).await?;
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

    async fn _install_deps(&self, deps: &[String], state: &ImageState) -> Result<()> {
        let span = info_span!("install-deps");
        async move {
            info!("installing dependencies");
            let pkg_mngr = state.os.package_manager();

            if deps.is_empty() {
                trace!("no dependencies to install");
                return Ok(());
            }

            trace!(deps = ?deps, "resolved dependency names");
            let deps = deps.join(" ");

            let cmd = [pkg_mngr.as_ref(), &pkg_mngr.install_args().join(" "), &deps].join(" ");
            trace!(command = %cmd, "installing with");

            self.checked_exec(&cmd).await.map(|_| ())
        }
        .instrument(span)
        .await
    }
}
