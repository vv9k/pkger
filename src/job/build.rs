use crate::cleanup;
use crate::image::{Image, ImageState, ImagesState};
use crate::job::{container::DockerContainer, Ctx, JobCtx};
use crate::recipe::{BuildTarget, Recipe};
use crate::util::{create_tar_archive, save_tar_gz, unpack_archive};
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
            .create_package(&image_state, out_dir.as_path())
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
                deps.push("dpkg");
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

    pub async fn create_package(&self, image_state: &ImageState, output_dir: &Path) -> Result<()> {
        match self.target {
            BuildTarget::Rpm => self.build_rpm(&image_state, &output_dir).await,
            BuildTarget::Gzip => self.build_gzip(&output_dir).await,
            BuildTarget::Deb => self.build_deb(&image_state, &output_dir).await,
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
            .map(|_| ())
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

    /// Creates a final RPM package and saves it to `output_dir`
    async fn build_rpm(&self, image_state: &ImageState, output_dir: &Path) -> Result<()> {
        let span = info_span!("RPM", container = %self.container.id());
        let _enter = span.enter();

        info!(parent: &span, "building RPM package");

        let name = [
            &self.recipe.metadata.name,
            "-",
            &self.recipe.metadata.version,
        ]
        .join("");
        let revision = if &self.recipe.metadata.revision == "" {
            "0"
        } else {
            &self.recipe.metadata.revision
        };
        let arch = if &self.recipe.metadata.arch == "" {
            "noarch"
        } else {
            &self.recipe.metadata.arch
        };
        let buildroot_name = [&name, "-", &revision, ".", &arch].join("");
        let source_tar = [&name, ".tar.gz"].join("");

        let base_path = PathBuf::from("/root/rpmbuild");
        let specs = base_path.join("SPECS");
        let sources = base_path.join("SOURCES");
        let rpms = base_path.join("RPMS");
        let rpms_arch = rpms.join(&arch);
        let srpms = base_path.join("SRPMS");
        let tmp_buildroot = PathBuf::from(["/tmp/", &buildroot_name].join(""));
        let source_tar_path = sources.join(&source_tar);

        let dirs = [
            specs.as_path(),
            sources.as_path(),
            rpms.as_path(),
            rpms_arch.as_path(),
            srpms.as_path(),
        ];

        self.create_dirs(&dirs[..]).instrument(span.clone()).await?;

        trace!(parent: &span, "copy source files to temporary location");
        self.container
            .exec(format!(
                "cp -rv {} {}",
                self.container_out_dir.display(),
                tmp_buildroot.display(),
            ))
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "prepare archived source files");
        self.container
            .exec(format!(
                "cd {} && tar -zcvf {} .",
                tmp_buildroot.display(),
                source_tar_path.display(),
            ))
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "find source file paths");
        let files = self
            .container
            .exec(format!(
                r#"cd {} && find . -type f -name "*""#,
                self.container_out_dir.display()
            ))
            .instrument(span.clone())
            .await
            .map(|out| {
                out.stdout
                    .join("")
                    .split_ascii_whitespace()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.trim_start_matches('.').to_string())
                    .collect::<Vec<_>>()
            })?;
        trace!(source_files = %files.join(", "));

        let spec = self
            .recipe
            .as_rpm_spec(&[source_tar], &files[..], &image_state.image)
            .render_owned()?;

        // this should be handled by rpmspec-rs
        let mut lines = spec.lines();
        let mut spec_new = String::new();
        while let Some(line) = lines.next() {
            if line.starts_with("%description") {
                spec_new.push('\n');
                spec_new.push_str(line);
                spec_new.push('\n');
                break;
            } else if line == "" {
                continue;
            } else {
                spec_new.push_str(line);
                spec_new.push('\n');
            }
        }
        lines.for_each(|line| {
            spec_new.push_str(line);
            spec_new.push('\n');
        });
        let spec = spec_new;

        let spec_file = [&self.recipe.metadata.name, ".spec"].join("");
        debug!(parent: &span, spec_file = %spec_file, spec = %spec);

        let entries = vec![(["./", &spec_file].join(""), spec.as_bytes())];
        let spec_tar = async move { create_tar_archive(entries) }
            .instrument(span.clone())
            .await?;

        let spec_tar_path = specs.join([&name, "-spec.tar"].join(""));

        trace!(parent: &span, "copy spec archive to container");
        self.container
            .inner()
            .copy_file_into(spec_tar_path.as_path(), &spec_tar)
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "extract spec archive");
        self.container
            .exec(format!(
                "tar -xvf {} -C {}",
                spec_tar_path.display(),
                specs.display(),
            ))
            .instrument(span.clone())
            .await?;

        // TODO: check why rpmbuild doesn't extract the source_tar to BUILDROOT
        self.container
            .exec(format!("rpmbuild -bb {}", specs.join(spec_file).display(),))
            .instrument(span.clone())
            .await?;
        // TODO: verify stderr here to check if build succeded

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

    /// Creates a final DEB packages and saves it to `output_dir`
    async fn build_deb(&self, image_state: &ImageState, output_dir: &Path) -> Result<()> {
        let span = info_span!("DEB", container = %self.container.id());
        let _enter = span.enter();

        info!(parent: &span, "building DEB package");

        let name = [
            &self.recipe.metadata.name,
            "-",
            &self.recipe.metadata.version,
        ]
        .join("");

        let debbld_dir = PathBuf::from("/root/debbuild");
        let tmp_dir = debbld_dir.join("tmp");
        let base_dir = debbld_dir.join(&name);
        let deb_dir = base_dir.join("DEBIAN");
        let dirs = [deb_dir.as_path(), tmp_dir.as_path()];

        self.create_dirs(&dirs[..]).instrument(span.clone()).await?;

        let control = self
            .recipe
            .as_deb_control(&image_state.image)
            .render_owned()?;
        debug!(parent: &span, control = %control);

        let entries = vec![("./control", control.as_bytes())];
        let control_tar = async move { create_tar_archive(entries) }
            .instrument(span.clone())
            .await?;
        let control_tar_path = tmp_dir.join([&name, "-control.tar"].join(""));

        trace!(parent: &span, "copy control archive to container");
        self.container
            .inner()
            .copy_file_into(control_tar_path.as_path(), &control_tar)
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "extract control archive");
        self.container
            .exec(format!(
                "tar -xvf {} -C {}",
                control_tar_path.display(),
                deb_dir.display(),
            ))
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "copy source files to build dir");
        self.container
            .exec(format!(
                "cp -r {}/ {}",
                self.container_out_dir.display(),
                base_dir.display()
            ))
            .instrument(span.clone())
            .await?;

        self.container
            .exec(format!(
                "dpkg-deb --build --root-owner-group {}",
                base_dir.display()
            ))
            .instrument(span.clone())
            .await?;

        let deb = self
            .container
            .inner()
            .copy_from(debbld_dir.join([&name, ".deb"].join("")).as_path())
            .try_concat()
            .instrument(span.clone())
            .await?;

        let mut archive = tar::Archive::new(&deb[..]);

        async move {
            unpack_archive(&mut archive, output_dir)
                .map_err(|e| anyhow!("failed to unpack archive - {}", e))
        }
        .instrument(span.clone())
        .await
    }

    /// Creates a final GZIP package and saves it to `output_dir`
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

        let cmd = [pkg_mngr.as_ref(), &pkg_mngr.install_args().join(" "), &deps].join(" ");
        trace!(command = %cmd, "installing with");

        self.container
            .exec(cmd)
            .instrument(span.clone())
            .await
            .map(|_| ())
    }
}
