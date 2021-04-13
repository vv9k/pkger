use crate::cleanup;
use crate::image::{Image, ImageState, ImagesState};
use crate::job::{
    container::{BuildContainerCtx, CONTAINER_ID_LEN},
    Ctx, JobCtx,
};
use crate::recipe::{BuildTarget, Recipe};
use crate::Config;
use crate::Result;

use async_trait::async_trait;
use futures::StreamExt;
use moby::{image::ImageBuildChunk, BuildOptions, ContainerOptions, Docker};
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
    verbose: bool,
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

        cleanup!(container, span);

        container
            .install_deps(&image_state)
            .instrument(span.clone())
            .await?;

        cleanup!(container, span);

        container.execute_scripts().instrument(span.clone()).await?;

        cleanup!(container, span);

        container.cleanup().await?;

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
        verbose: bool,
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
            verbose,
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

        trace!(opts = ?opts);

        let id = self
            .docker
            .containers()
            .create(&opts)
            .instrument(span.clone())
            .await
            .map(|info| info.id)?;
        info!(container_id = %id[..CONTAINER_ID_LEN], "created container");
        let container = self.docker.containers().get(&id);

        container.start().instrument(span.clone()).await?;
        info!(container_id = %id[..CONTAINER_ID_LEN], "started container");

        Ok(BuildContainerCtx::new(
            container,
            &self.recipe,
            &self.image,
            self.is_running.clone(),
            self.target.clone(),
            self.bld_dir.as_path(),
            self.out_dir.as_path(),
        ))
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

    //async fn archive_output_dir<'job>(&self, container: &Container<'job>) -> Result<()> {}
}

impl<'job> From<BuildCtx> for JobCtx<'job> {
    fn from(ctx: BuildCtx) -> Self {
        JobCtx::Build(ctx)
    }
}
