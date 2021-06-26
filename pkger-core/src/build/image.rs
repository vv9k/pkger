use crate::build::{container, deps, Context};
use crate::docker::{image::ImageBuildChunk, BuildOptions, Docker};
use crate::image::{Image, ImageState, ImagesState};
use crate::recipe::RecipeTarget;
use crate::{Error, Result};

use futures::StreamExt;
use std::collections::HashSet;
use std::fs;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tempdir::TempDir;
use tracing::{debug, info, info_span, trace, warn, Instrument};

pub static CACHED: &str = "cached";
pub static LATEST: &str = "latest";

impl Context {
    pub async fn image_build(&mut self) -> Result<ImageState> {
        let span = info_span!("image-build");
        let cloned_span = span.clone();

        async move {
            let mut deps = if let Some(deps) = &self.recipe.metadata.build_depends {
                deps.resolve_names(&self.image.name)
            } else {
                Default::default()
            };
            deps.extend(deps::pkger_deps(self.target.build_target(), &self.recipe));
            trace!(resolved_deps = ?deps);

            let result = cloned_span.in_scope(|| {
                find_cached_state(&self.image, &self.target, &self.image_state, self.simple)
            });

            if let Some(state) = result {
                let state_deps = state
                    .deps
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<HashSet<_>>();
                if deps != state_deps {
                    info!(old = ?state.deps, new = ?deps, "dependencies changed");
                } else {
                    trace!("unchanged");

                    if state.exists(&self.docker).await {
                        trace!("state exists in docker");
                        return Ok(state);
                    } else {
                        warn!("found cached state but image doesn't exist in docker")
                    }
                }
            }

            debug!(image = %self.image.name, "building from scratch");
            let images = self.docker.images();
            let opts = BuildOptions::builder(self.image.path.to_string_lossy().to_string())
                .tag(&format!("{}:{}", &self.image.name, LATEST))
                .build();

            let mut stream = images.build(&opts);

            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                match chunk {
                    ImageBuildChunk::Error {
                        error,
                        error_detail: _,
                    } => {
                        return Err(Error::msg(error));
                    }
                    ImageBuildChunk::Update { stream } => {
                        info!("{}", stream);
                    }
                    ImageBuildChunk::Digest { aux } => {
                        let state = ImageState::new(
                            &aux.id,
                            &self.target,
                            LATEST,
                            &SystemTime::now(),
                            &self.docker,
                            &Default::default(),
                            self.simple,
                        )
                        .await?;

                        if let Ok(mut image_state) = self.image_state.write() {
                            (*image_state).update(&self.target, &state)
                        }

                        return Ok(state);
                    }
                    _ => {}
                }
            }

            Err(Error::msg("stream ended before image id was received"))
        }
        .instrument(span)
        .await
    }
}

pub async fn cache_image(
    ctx: &container::Context<'_>,
    docker: &Docker,
    state: &ImageState,
    deps: &HashSet<&str>,
) -> Result<ImageState> {
    let span = info_span!("cache-image", image = %state.image);
    async move {
        let pkg_mngr = state.os.package_manager();
        let pkg_mngr_name = pkg_mngr.as_ref();
        let tag = format!("{}:{}", state.image, state.tag);

        if pkg_mngr_name.is_empty() {
            return Err(Error::msg(format!(
                "caching image failed - no package manger found for os `{}`",
                state.os.name()
            )));
        }

        let deps_joined = deps.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        #[rustfmt::skip]
            let dockerfile = format!(
r#"FROM {}
RUN {} {}
RUN {} {} {} >/dev/null"#,
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
                    return Err(Error::msg(error));
                }
                ImageBuildChunk::Update { stream } => {
                    info!("{}", stream);
                }
                ImageBuildChunk::Digest { aux } => {
                    return ImageState::new(
                        &aux.id,
                        &ctx.target,
                        CACHED,
                        &SystemTime::now(),
                        &docker,
                        deps,
                        ctx.simple,
                    )
                    .await
                }
                _ => {}
            }
        }

        Err(Error::msg("id of image not received"))
    }
    .instrument(span)
    .await
}

/// Checks whether any of the files located at the path of this Image changed since last build.
/// If shouldn't be rebuilt returns previous `ImageState`.
pub fn find_cached_state(
    image: &Image,
    target: &RecipeTarget,
    state: &Arc<RwLock<ImagesState>>,
    simple: bool,
) -> Option<ImageState> {
    let span = info_span!("find-image-cache");
    let _enter = span.enter();

    trace!(recipe = ?target);

    trace!("checking if image should be rebuilt");
    if let Ok(states) = state.read() {
        if let Some(state) = (*states).images.get(&target) {
            if simple {
                return Some(state.to_owned());
            }
            if let Ok(entries) = fs::read_dir(image.path.as_path()) {
                for file in entries {
                    if let Err(e) = file {
                        warn!(reason = %e, "error while loading file");
                        continue;
                    }
                    let file = file.unwrap();
                    let path = file.path();
                    trace!(path = %path.display(), "checking");
                    let metadata = fs::metadata(path.as_path());
                    if let Err(e) = metadata {
                        warn!(
                            path = %path.display(),
                            reason = %e,
                            "failed to read metadata",
                        );
                        continue;
                    }
                    let metadata = metadata.unwrap();
                    let mod_time = metadata.modified();
                    if let Err(e) = &mod_time {
                        warn!(
                            path = %path.display(),
                            reason = %e,
                            "failed to check modification time",
                        );
                        continue;
                    }
                    let mod_time = mod_time.unwrap();
                    if mod_time > state.timestamp {
                        trace!(mod_time = ?mod_time, image_mod_time = ?state.timestamp, "found modified file, not returning cache");
                        return None;
                    }
                }
            }
            let state = state.to_owned();
            trace!(image_state = ?state, "found cached state");
            return Some(state);
        }
    }
    None
}
