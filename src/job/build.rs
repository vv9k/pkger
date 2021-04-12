use crate::image::{Image, ImageState, ImagesState};
use crate::job::{Ctx, JobCtx};
use crate::recipe::{BuildTarget, Recipe};
use crate::Config;
use crate::Result;

use futures::StreamExt;
use moby::{
    image::ImageBuildChunk, tty::TtyChunk, BuildOptions, Container, ContainerOptions, Docker,
    ExecContainerOptions, RmContainerOptions,
};
use std::path::PathBuf;
use std::str;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use tracing::{debug, error, info, info_span, trace, Instrument};

#[derive(Debug)]
/// Groups all data and functionality necessary to create an artifact
pub struct BuildCtx {
    id: String,
    recipe: Recipe,
    image: Image,
    config: Arc<Config>,
    docker: Arc<Docker>,
    image_state: Arc<RwLock<ImagesState>>,
    is_running: Arc<AtomicBool>,
    bld_dir: PathBuf,
    out_dir: PathBuf,
    target: BuildTarget,
    verbose: bool,
}

impl Ctx for BuildCtx {
    fn id(&self) -> &str {
        &self.id
    }
}

macro_rules! cleanup {
    ($ctx:ident, $container:ident, $span: ident) => {
        if $ctx
            .cleanup_if_exit(&$container)
            .instrument($span.clone())
            .await?
        {
            return Err(anyhow!("job interrupted by ctrl-c signal"));
        }
    };
}

impl BuildCtx {
    pub fn new(
        recipe: Recipe,
        image: Image,
        config: Arc<Config>,
        docker: Arc<Docker>,
        image_state: Arc<RwLock<ImagesState>>,
        is_running: Arc<AtomicBool>,
        target: BuildTarget,
        verbose: bool,
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
            config,
            image,
            recipe,
            docker,
            image_state,
            is_running,
            bld_dir,
            out_dir,
            target,
            verbose,
        }
    }

    async fn cleanup_if_exit<'job>(&self, container: &Container<'job>) -> Result<bool> {
        let span = info_span!("check-is-running");
        let _enter = span.enter();
        if !self.is_running.load(Ordering::SeqCst) {
            trace!(container_id = %container.id(), "not running, cleanup");

            container
                .stop(None)
                .instrument(span.clone())
                .await
                .map_err(|e| anyhow!("failed to stop container - {}", e))?;

            return container
                .delete()
                .instrument(span.clone())
                .await
                .map_err(|e| anyhow!("failed to delete container - {}", e))
                .map(|_| true);
        }

        Ok(false)
    }

    pub async fn run(&mut self) -> Result<()> {
        let span =
            info_span!("build", recipe = %self.recipe.metadata.name, image = %self.image.name);
        let _enter = span.enter();

        if self.verbose {
            info!(id = %self.id, "running job" );
        }
        let image_state = self
            .image_build()
            .instrument(span.clone())
            .await
            .map_err(|e| anyhow!("failed to build image - {}", e))?;

        if self.verbose {
            info!(image = %image_state.image);
        }

        let container = self
            .container_spawn(&image_state)
            .instrument(span.clone())
            .await?;

        cleanup!(self, container, span);

        self.install_deps(&container, &image_state)
            .instrument(span.clone())
            .await?;

        cleanup!(self, container, span);

        self.execute_scripts(&container)
            .instrument(span.clone())
            .await?;

        cleanup!(self, container, span);

        if let Err(e) = container
            .remove(
                &RmContainerOptions::builder()
                    .force(true)
                    .volumes(true)
                    .build(),
            )
            .instrument(span.clone())
            .await
        {
            error!("failed to delete container - {}", e);
        }

        Ok(())
    }

    /// Creates and starts a container from the given ImageState
    async fn container_spawn<'job>(
        &'job self,
        image_state: &ImageState,
    ) -> Result<Container<'job>> {
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
            .env(env.to_kv_vec())
            .working_dir(self.bld_dir.to_string_lossy().to_string().as_str())
            .build();

        trace!(opts = ?opts);

        let id = self
            .docker
            .containers()
            .create(&opts)
            .instrument(span.clone())
            .await
            .map(|info| info.id)?;
        info!(container_id = %id, "created container");
        let container = self.docker.containers().get(&id);

        container.start().instrument(span.clone()).await?;
        info!(container_id = %id, "started container");

        Ok(container)
    }

    async fn container_exec<'job, S: AsRef<str>>(
        &self,
        container: &Container<'job>,
        cmd: S,
    ) -> Result<()> {
        let span = info_span!("container-exec");
        let _enter = span.enter();

        debug!(cmd = %cmd.as_ref(), container = %container.id(), "executing");

        let opts = ExecContainerOptions::builder()
            .cmd(vec!["/bin/sh", "-c", cmd.as_ref()])
            .attach_stdout(true)
            .attach_stderr(true)
            .build();

        let mut stream = container.exec(&opts);

        while let Some(result) = stream.next().instrument(span.clone()).await {
            cleanup!(self, container, span);
            match result? {
                TtyChunk::StdOut(chunk) => {
                    if self.verbose {
                        info!("{}", str::from_utf8(&chunk)?.trim_end_matches("\n"));
                    }
                }
                TtyChunk::StdErr(chunk) => {
                    if self.verbose {
                        error!("{}", str::from_utf8(&chunk)?.trim_end_matches("\n"));
                    }
                }
                _ => unreachable!(),
            }
        }
        Ok(())
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
                    return Err(anyhow!(error.to_string()));
                }
                ImageBuildChunk::Update { stream } => {
                    if self.verbose {
                        info!("{}", stream);
                    }
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

    async fn install_deps<'job>(
        &self,
        container: &Container<'job>,
        state: &ImageState,
    ) -> Result<()> {
        let span = info_span!("install-deps");
        let _enter = span.enter();

        info!("installing dependencies");
        let pkg_mngr = state.os.package_manager();
        let deps = if let Some(deps) = &self.recipe.metadata.build_depends {
            deps.resolve_names(&state.image)
        } else {
            vec![]
        }
        .join(" ");
        trace!(deps = %deps, "resolved dependency names");

        let cmd = format!(
            "{} {} {}",
            pkg_mngr.as_ref(),
            pkg_mngr.install_args().join(" "),
            deps,
        );
        trace!(command = %cmd, "installing with");

        self.container_exec(container, cmd)
            .instrument(span.clone())
            .await
    }

    async fn execute_scripts<'job>(&self, container: &Container<'job>) -> Result<()> {
        let span = info_span!("exec-scripts");
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
                self.container_exec(container, &cmd.cmd)
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
            self.container_exec(container, &cmd.cmd)
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
                self.container_exec(container, &cmd.cmd)
                    .instrument(span.clone())
                    .await?;
            }
        }

        Ok(())
    }
}

impl<'job> From<BuildCtx> for JobCtx<'job> {
    fn from(ctx: BuildCtx) -> Self {
        JobCtx::Build(ctx)
    }
}
