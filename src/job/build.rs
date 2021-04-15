use crate::cleanup;
use crate::image::{Image, ImageState, ImagesState};
use crate::job::{container::DockerContainer, Ctx, JobCtx};
use crate::recipe::{BuildTarget, Recipe};
use crate::util::{save_tar_gz, unpack_archive};
use crate::Config;
use crate::Result;

use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};
use moby::{image::ImageBuildChunk, BuildOptions, ContainerOptions, Docker};
use rpmspec::RpmSpec;
use std::fs;
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

        let out_dir = self
            .create_out_dir(&image_state)
            .instrument(span.clone())
            .await?;

        let container_ctx = self
            .container_spawn(&image_state)
            .instrument(span.clone())
            .await?;

        cleanup!(container_ctx, span);

        let skip_deps = self.recipe.metadata.skip_default_deps.unwrap_or(false);

        if !skip_deps {
            container_ctx
                .install_pkger_deps(&image_state)
                .instrument(span.clone())
                .await?;

            cleanup!(container_ctx, span);
        }

        container_ctx
            .install_recipe_deps(&image_state)
            .instrument(span.clone())
            .await?;

        cleanup!(container_ctx, span);

        let dirs = vec![
            self.container_out_dir.to_string_lossy().to_string(),
            self.container_bld_dir.to_string_lossy().to_string(),
        ];

        container_ctx
            .create_dirs(&dirs[..])
            .instrument(span.clone())
            .await?;

        cleanup!(container_ctx, span);

        container_ctx
            .execute_scripts()
            .instrument(span.clone())
            .await?;

        cleanup!(container_ctx, span);

        container_ctx
            .create_package(out_dir.as_path())
            .instrument(span.clone())
            .await?;

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
        let span = info_span!("container-spawn");
        let _enter = span.enter();

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

    async fn create_out_dir(&self, image: &ImageState) -> Result<PathBuf> {
        let span = info_span!("install-deps");
        let _enter = span.enter();

        let os_ver = image.os.os_ver();
        let out_dir = self
            .out_dir
            .join(format!("{}/{}", image.os.as_ref(), os_ver));

        if out_dir.exists() {
            trace!(dir = %out_dir.display(), "already exists");
            Ok(out_dir)
        } else {
            trace!(dir = %out_dir.display(), "creating output directory");
            fs::create_dir_all(out_dir.as_path())
                .map(|_| out_dir)
                .map_err(|e| anyhow!("failed to create output directory - {}", e))
        }
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
    container_out_dir: &'job Path,
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

    pub async fn check_is_running(&self) -> Result<bool> {
        self.container.check_is_running().await
    }

    pub async fn start_container(&mut self) -> Result<()> {
        self.container.spawn(&self.opts).await
    }

    pub async fn install_recipe_deps(&self, state: &ImageState) -> Result<()> {
        let span = info_span!("install-recipe-deps", container = %self.container.id());
        let _enter = span.enter();

        let deps = if let Some(deps) = &self.recipe.metadata.build_depends {
            deps.resolve_names(&state.image)
        } else {
            vec![]
        };

        self._install_deps(&deps, &state)
            .instrument(span.clone())
            .await
    }

    pub async fn install_pkger_deps(&self, state: &ImageState) -> Result<()> {
        let span = info_span!("install-default-deps", container = %self.container.id());
        let _enter = span.enter();

        let mut deps = vec!["tar", "git"];
        match self.target {
            BuildTarget::Rpm => {
                deps.push("rpm-build");
            }
            BuildTarget::Deb => {
                deps.push("dpkg-deb");
            }
            BuildTarget::Gzip => {
                deps.push("gzip");
            }
        }

        let deps = deps.into_iter().map(str::to_string).collect::<Vec<_>>();

        self._install_deps(&deps, &state)
            .instrument(span.clone())
            .await
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

    pub async fn create_package(&self, output_dir: &Path) -> Result<()> {
        match self.target {
            BuildTarget::Rpm => self.build_rpm(&output_dir).await,
            BuildTarget::Gzip => self.build_gzip(&output_dir).await,
            _ => Ok(()),
        }
    }

    pub async fn create_dirs<P: AsRef<Path>>(&self, dirs: &[P]) -> Result<()> {
        let span = info_span!("create-dirs", container = %self.container.id());
        let _enter = span.enter();

        let dirs_joined =
            dirs.iter()
                .map(P::as_ref)
                .fold(String::new(), |mut dirs_joined, path| {
                    dirs_joined.push_str(&format!(" {}", path.display()));
                    dirs_joined
                });
        let dirs_joined = dirs_joined.trim();

        trace!(directories = %dirs_joined);

        self.container
            .exec(format!("mkdir -pv {}", dirs_joined))
            .instrument(span.clone())
            .await
    }

    pub async fn archive_output_dir(&self) -> Result<Vec<u8>> {
        let span = info_span!("archive-output", container = %self.container.id());

        info!("copying final archive");
        self.container
            .inner()
            .copy_from(self.container_out_dir)
            .try_concat()
            .instrument(span.clone())
            .await
            .map_err(|e| anyhow!("failed to archive output directory - {}", e))
    }

    /// Creates final RPM package and saves it to `output_dir`
    async fn build_rpm(&self, output_dir: &Path) -> Result<()> {
        let span = info_span!("RPM", container = %self.container.id());
        let _enter = span.enter();

        info!(parent: &span, "building RPM package");

        let base_path = PathBuf::from("/root/rpmbuild");
        let specs = base_path.join("SPECS");
        let sources = base_path.join("SOURCES");
        let rpms = base_path.join("RPMS");
        let srpms = base_path.join("SRPMS");

        let dirs = vec![
            specs.as_path(),
            sources.as_path(),
            rpms.as_path(),
            srpms.as_path(),
        ];

        self.create_dirs(&dirs[..]).instrument(span.clone()).await?;

        let source_tar = format!(
            "{}-{}.tar.gz",
            &self.recipe.metadata.name, &self.recipe.metadata.version
        );

        self.container
            .exec(format!(
                "tar -zcvf {} {}",
                sources.join(&source_tar).display(),
                self.container_out_dir.display()
            ))
            .instrument(span.clone())
            .await?;

        let spec = RpmSpec::from(self.recipe).render_owned()?;
        let spec_file = format!("{}.spec", &self.recipe.metadata.name);
        debug!(parent: &span, spec = %spec);

        trace!(parent: &span, "create tar archive");
        let mut archive_buf = Vec::new();
        let mut archive = tar::Builder::new(&mut archive_buf);
        let mut header = tar::Header::new_gnu();
        header.set_size(spec.as_bytes().iter().count() as u64);
        header.set_cksum();
        archive.append_data(&mut header, &format!("./{}", spec_file), spec.as_bytes())?;
        let archive_buf = archive.into_inner()?;

        let spec_tar = specs.join(&spec_file);

        trace!(parent: &span, "copy archive to container");
        self.container
            .inner()
            .copy_file_into(spec_tar.as_path(), archive_buf)
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "extract archive");
        self.container
            .exec(format!(
                "tar -xvf {} -C {}",
                spec_tar.display(),
                specs.display(),
            ))
            .instrument(span.clone())
            .await?;

        self.container
            .exec(format!("rpmbuild -bb {}", specs.join(spec_file).display(),))
            .instrument(span.clone())
            .await?;

        let rpm = self
            .container
            .inner()
            .copy_from(rpms.join(&self.recipe.metadata.arch).as_path())
            .try_concat()
            .instrument(span.clone())
            .await?;

        let mut archive = tar::Archive::new(&rpm[..]);

        async move {
            unpack_archive(&mut archive, output_dir)
                .map_err(|e| anyhow!("failed to unpack archive - {}", e))
        }
        .instrument(span.clone())
        .await
    }

    async fn build_deb(&self, output_dir: &Path) -> Result<()> {
        let span = info_span!("DEB", container = %self.container.id());
        let _enter = span.enter();

        info!(parent: &span, "building DEB package");

        let base_dir = PathBuf::from(format!(
            "/root/debbuild/{}-{}",
            &self.recipe.metadata.name, &self.recipe.metadata.version
        ));
        let dirs = vec![base_dir.join("DEBIAN")];

        self.create_dirs(&dirs[..]).instrument(span.clone()).await?;

        Ok(())
    }

    /// Creates final GZIP package and saves it to `output_dir`
    async fn build_gzip(&self, output_dir: &Path) -> Result<()> {
        let span = info_span!("GZIP", container = %self.container.id());
        let _enter = span.enter();

        info!(parent: &span, "building GZIP package");
        let package = self
            .container
            .inner()
            .copy_from(self.container_out_dir)
            .try_concat()
            .instrument(span.clone())
            .await?;

        let archive = tar::Archive::new(&package[..]);

        async move {
            save_tar_gz(
                archive,
                &format!(
                    "{}-{}.tar.gz",
                    &self.recipe.metadata.name, &self.recipe.metadata.version
                ),
                output_dir,
            )
            .map_err(|e| anyhow!("failed to save package as tar.gz - {}", e))
        }
        .instrument(span.clone())
        .await
    }

    async fn _install_deps(&self, deps: &[String], state: &ImageState) -> Result<()> {
        let span = info_span!("install-deps");
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
