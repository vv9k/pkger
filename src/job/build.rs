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
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use tracing::{error, event, info, span, Instrument, Level};

#[derive(Debug)]
pub struct BuildCtx {
    id: String,
    recipe: Recipe,
    image: Image,
    _config: Arc<Config>,
    docker: Arc<Docker>,
    image_state: Arc<RwLock<ImagesState>>,
    bld_dir: PathBuf,
    _target: BuildTarget,
    verbose: bool,
}

impl Ctx for BuildCtx {
    fn id(&self) -> &str {
        &self.id
    }
}

impl BuildCtx {
    pub fn new(
        recipe: Recipe,
        image: Image,
        config: Arc<Config>,
        docker: Arc<Docker>,
        image_state: Arc<RwLock<ImagesState>>,
        _target: BuildTarget,
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
        let bld_dir = PathBuf::from(format!("/tmp/{}-{}", &recipe.metadata.name, &timestamp,));
        event!(Level::TRACE, id = %id, "creating new build context");

        BuildCtx {
            id,
            _config: config,
            image,
            recipe,
            docker,
            image_state,
            bld_dir,
            _target,
            verbose,
        }
    }

    // If successful returns id of the container
    async fn container_spawn(&self, image_state: &ImageState) -> Result<String> {
        let span = span!(Level::INFO, "container-spawn");
        let _enter = span.enter();

        let mut env = self.recipe.env.clone();
        env.insert("PKGER_BLD_DIR", self.bld_dir.to_string_lossy());
        env.insert("PKGER_OS", image_state.os.as_ref());
        env.insert("PKGER_OS_VERSION", image_state.os.os_ver());

        event!(parent: &span, Level::DEBUG, env = ?env);

        let opts = ContainerOptions::builder(&image_state.image)
            .name(&self.id)
            .cmd(vec!["sleep infinity"])
            .entrypoint(vec!["/bin/sh", "-c"])
            .env(env.to_kv_vec())
            .working_dir(self.bld_dir.to_string_lossy().to_string().as_str())
            .build();

        event!(parent: &span, Level::DEBUG, opts = ?opts);

        Ok(self
            .docker
            .containers()
            .create(&opts)
            .instrument(span.clone())
            .await
            .map(|info| info.id)?)
    }

    async fn container_exec<'a, S: AsRef<str>>(
        &self,
        container: &Container<'a>,
        cmd: S,
    ) -> Result<()> {
        let span = span!(Level::INFO, "container-exec");
        let _enter = span.enter();

        event!(parent: &span, Level::DEBUG, cmd = %cmd.as_ref(), container = %container.id(), "executing");

        let opts = ExecContainerOptions::builder()
            .cmd(vec!["/bin/sh", "-c", cmd.as_ref()])
            .attach_stdout(true)
            .attach_stderr(true)
            .build();

        let mut stream = container.exec(&opts);

        while let Some(result) = stream.next().instrument(span.clone()).await {
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
        let span = span!(Level::INFO, "image-build");
        let _enter = span.enter();

        if let Some(state) = self.image.find_cached_state(&self.image_state) {
            event!(parent: &span, Level::DEBUG, state = ?state, "found cached image state");
            return Ok(state);
        }

        event!(parent: &span, Level::DEBUG, image = %self.image.name, "building from scratch");
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

    async fn install_deps(&self, container: &Container<'_>, state: &ImageState) -> Result<()> {
        let span = span!(Level::INFO, "install-deps");
        let _enter = span.enter();

        event!(parent: &span, Level::INFO, "installing dependencies");

        let pkg_mngr = state.os.package_manager();
        let deps = if let Some(deps) = &self.recipe.metadata.build_depends {
            deps.resolve_names(&state.image)
        } else {
            vec![]
        }
        .join(" ");
        event!(parent: &span, Level::DEBUG, deps = %deps, "resolved dependency names");

        let cmd = format!(
            "{} {} {}",
            pkg_mngr.as_ref(),
            pkg_mngr.install_args().join(" "),
            deps,
        );
        event!(parent: &span, Level::DEBUG, command = %cmd, "installing with");

        self.container_exec(&container, cmd)
            .instrument(span.clone())
            .await
    }

    pub async fn run(&mut self) -> Result<()> {
        let span = span!(Level::INFO, "build", recipe = %self.recipe.metadata.name, image = %self.image.name);
        let _enter = span.enter();

        if self.verbose {
            event!(parent: &span, Level::INFO, id = %self.id, "running job" );
        }
        let image_state = self
            .image_build()
            .instrument(span.clone())
            .await
            .map_err(|e| anyhow!("failed to build image - {}", e))?;

        if self.verbose {
            event!(parent: &span, Level::INFO, image = %image_state.image);
        }

        let id = self
            .container_spawn(&image_state)
            .instrument(span.clone())
            .await?;

        let containers = self.docker.containers();
        let container = containers.get(&id);
        event!(parent: &span, Level::INFO, container_id = %id, "created container");

        container.start().instrument(span.clone()).await?;
        event!(parent: &span, Level::INFO, container_id = %id, "started container");

        self.install_deps(&container, &image_state)
            .instrument(span.clone())
            .await?;

        if let Some(config_script) = &self.recipe.configure_script {
            for cmd in &config_script.steps {
                if !cmd.images.is_empty() {
                    if !cmd.images.contains(&self.image.name) {
                        continue;
                    }
                }
                self.container_exec(&container, &cmd.cmd)
                    .instrument(span.clone())
                    .await?;
            }
        }

        for cmd in &self.recipe.build_script.steps {
            if !cmd.images.is_empty() {
                if !cmd.images.contains(&self.image.name) {
                    continue;
                }
            }
            self.container_exec(&container, &cmd.cmd)
                .instrument(span.clone())
                .await?;
        }

        if let Some(install_script) = &self.recipe.install_script {
            for cmd in &install_script.steps {
                if !cmd.images.is_empty() {
                    if !cmd.images.contains(&self.image.name) {
                        continue;
                    }
                }
                self.container_exec(&container, &cmd.cmd)
                    .instrument(span.clone())
                    .await?;
            }
        }

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
}

impl<'j> From<BuildCtx> for JobCtx<'j> {
    fn from(ctx: BuildCtx) -> Self {
        JobCtx::Build(ctx)
    }
}
