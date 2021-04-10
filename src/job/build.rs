use crate::image::{Image, ImageState, ImagesState};
use crate::job::JobCtx;
use crate::recipe::{BuildTarget, Recipe};
use crate::Config;
use crate::Result;

use futures::StreamExt;
use log::{debug, error, info};
use moby::{
    image::ImageBuildChunk, tty::TtyChunk, BuildOptions, Container, ContainerOptions, Docker,
    ExecContainerOptions, RmContainerOptions,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::str;
use std::time::SystemTime;

pub struct BuildCtx<'j> {
    id: String,
    _config: &'j Config,
    image: &'j Image,
    recipe: &'j Recipe,
    docker: &'j Docker,
    image_state: &'j RefCell<ImagesState>,
    bld_dir: PathBuf,
    _target: BuildTarget,
    verbose: bool,
}
impl<'j> BuildCtx<'j> {
    pub fn new(
        config: &'j Config,
        image: &'j Image,
        recipe: &'j Recipe,
        docker: &'j Docker,
        image_state: &'j RefCell<ImagesState>,
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
        debug!("{}", id);
        let bld_dir = PathBuf::from(format!("/tmp/{}-{}", &recipe.metadata.name, &timestamp,));

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
        let mut env = self.recipe.env.clone();
        env.insert("PKGER_BLD_DIR", self.bld_dir.to_string_lossy());
        env.insert("PKGER_OS", image_state.os.as_ref());
        env.insert("PKGER_OS_VERSION", image_state.os.os_ver());
        debug!("{:?}", &env);

        Ok(self
            .docker
            .containers()
            .create(
                &ContainerOptions::builder(&image_state.image)
                    .name(&self.id)
                    .cmd(vec!["sleep infinity"])
                    .entrypoint(vec!["/bin/sh", "-c"])
                    .env(env.to_kv_vec())
                    .working_dir(self.bld_dir.to_string_lossy().to_string().as_str())
                    .build(),
            )
            .await
            .map(|info| info.id)?)
    }

    async fn container_exec<S: AsRef<str>>(&self, container: &Container<'j>, cmd: S) -> Result<()> {
        let opts = ExecContainerOptions::builder()
            .cmd(vec!["/bin/sh", "-c", cmd.as_ref()])
            .attach_stdout(true)
            .attach_stderr(true)
            .build();

        let mut stream = container.exec(&opts);

        while let Some(result) = stream.next().await {
            match result? {
                TtyChunk::StdOut(chunk) => {
                    if self.verbose {
                        info!("{}", str::from_utf8(&chunk)?);
                    }
                }
                TtyChunk::StdErr(chunk) => {
                    if self.verbose {
                        error!("{}", str::from_utf8(&chunk)?);
                    }
                }
                _ => unreachable!(),
            }
        }
        Ok(())
    }

    async fn image_build(&mut self) -> Result<ImageState> {
        if !self.image.should_be_rebuilt(&self.image_state) {
            if let Some(image) = self
                .image_state
                .borrow()
                .images
                .get(&self.image.name)
                .cloned()
            {
                debug!("not rebuilding image, cache: {:#?}", image);
                return Ok(image);
            }
        }

        debug!("building image {}", &self.image.name);
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
                    .await?;

                    self.image_state
                        .borrow_mut()
                        .update(&self.image.name, &state);

                    return Ok(state);
                }
                _ => {}
            }
        }

        Err(anyhow!("stream ended before image id was received"))
    }

    pub async fn run(&mut self) -> Result<()> {
        if self.verbose {
            info!("running job {}", &self.id);
        }
        let image_state = self
            .image_build()
            .await
            .map_err(|e| anyhow!("failed to build image - {}", e))?;

        if self.verbose {
            info!("image: {}", image_state.image);
        }

        let id = self.container_spawn(&image_state).await?;
        let containers = self.docker.containers();
        let container = containers.get(&id);
        if self.verbose {
            info!("container id: {}", id);
        }

        info!("starting container");
        container.start().await?;

        for cmd in &self.recipe.build.steps {
            if !cmd.images.is_empty() {
                if !cmd.images.contains(&self.image.name) {
                    continue;
                }
            }
            self.container_exec(&container, &cmd.cmd).await?;
        }

        if let Err(e) = container
            .remove(
                &RmContainerOptions::builder()
                    .force(true)
                    .volumes(true)
                    .build(),
            )
            .await
        {
            error!("failed to delete container - {}", e);
        }

        Ok(())
    }
}

impl<'j> From<BuildCtx<'j>> for JobCtx<'j> {
    fn from(ctx: BuildCtx<'j>) -> Self {
        JobCtx::Build(ctx)
    }
}
