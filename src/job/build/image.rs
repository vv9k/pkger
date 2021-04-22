use crate::image::ImageState;
use crate::job::build::deps;
use crate::job::build::BuildContainerCtx;
use crate::job::BuildCtx;
use crate::Result;

use futures::StreamExt;
use moby::{image::ImageBuildChunk, BuildOptions, Docker};
use std::collections::HashSet;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use tempdir::TempDir;
use tracing::{debug, info, info_span, trace, warn, Instrument};

impl BuildCtx {
    pub async fn image_build(&mut self) -> Result<ImageState> {
        let span = info_span!("image-build");

        async move {
            if let Some(state) = self.image.find_cached_state(&self.image_state) {
                if let Some(new_deps) = &self.recipe.metadata.build_depends {
                    let mut new_deps = new_deps.resolve_names(&state.image);
                    new_deps.extend(deps::pkger_deps(&self.target, &self.recipe));
                    if new_deps != state.deps {
                        info!(old = ?state.deps, new = ?new_deps, "dependencies changed");
                    } else {
                        trace!("unchanged");
                        if state.exists(&self.docker).await {
                            trace!("exists");
                            return Ok(state);
                        } else {
                            warn!("found cached state but image doesn't exist in docker")
                        }
                    }
                } else if state.deps.is_empty() && state.exists(&self.docker).await {
                    if state.exists(&self.docker).await {
                        trace!("exists");
                        return Ok(state);
                    } else {
                        warn!("found cached state but image doesn't exist in docker")
                    }
                }
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
                            &Default::default(),
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
}

impl<'job> BuildContainerCtx<'job> {
    pub async fn cache_image(
        &self,
        docker: &Docker,
        state: &ImageState,
        deps: &HashSet<String>,
    ) -> Result<ImageState> {
        let span = info_span!("cache-image", image = %state.image);
        async move {
            let pkg_mngr = state.os.package_manager();
            let pkg_mngr_name = pkg_mngr.as_ref();
            let tag = format!("{}:{}", state.image, state.tag);

            let deps_joined = deps.iter().map(|s| s.to_string()).collect::<Vec<_>>();

            #[rustfmt::skip]
            let dockerfile = format!(
                r#"
FROM {}
RUN {} {}
RUN {} {} {}
"#,
                tag,
                pkg_mngr_name, pkg_mngr.update_repos_args().join(" "),
                pkg_mngr_name, pkg_mngr.install_args().join(" "), deps_joined.join(" ")
            );
            trace!(dockerfile = %dockerfile);

            let temp = TempDir::new(&format!(
                "{}-cache-{}",
                state.image,
                state
                    .timestamp
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            ))?;
            let temp_path = temp.path();
            trace!(temp_dir = %temp_path.display());
            fs::write(temp_path.join("Dockerfile"), dockerfile)?;

            let images = docker.images();
            let opts = BuildOptions::builder(temp_path.to_string_lossy().to_string())
                .tag(tag)
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
                        return ImageState::new(
                            &aux.id,
                            &state.image,
                            "cached",
                            &SystemTime::now(),
                            &docker,
                            deps,
                        )
                        .await
                    }
                    _ => {}
                }
            }

            Err(anyhow!("id of image not received"))
        }
        .instrument(span)
        .await
    }
}
