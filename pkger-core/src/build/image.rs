use crate::build::{container, deps, Context};
use crate::docker::{
    api::{BuildOpts, ImageBuildChunk},
    Docker,
};
use crate::image::{ImageState, ImagesState};
use crate::recipe::RecipeTarget;
use crate::{Error, Result};

use futures::StreamExt;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tempdir::TempDir;
use tracing::{debug, info, info_span, trace, warn, Instrument};

pub static CACHED: &str = "cached";
pub static LATEST: &str = "latest";

pub async fn build(ctx: &mut Context) -> Result<ImageState> {
    let span = info_span!("image-build");
    let cloned_span = span.clone();

    async move {
        let mut deps = if let Some(deps) = &ctx.recipe.metadata.build_depends {
            deps.resolve_names(&ctx.target.image())
        } else {
            Default::default()
        };
        deps.extend(deps::pkger_deps(ctx.target.build_target(), &ctx.recipe));
        trace!(resolved_deps = ?deps);

        let result = cloned_span.in_scope(|| {
            find_cached_state(&ctx.image.path, &ctx.target, &ctx.image_state, ctx.simple)
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

                if state.exists(&ctx.docker).await {
                    trace!("state exists in docker");
                    return Ok(state);
                } else {
                    warn!("found cached state but image doesn't exist in docker")
                }
            }
        }

        debug!(image = %ctx.target.image(), "building from scratch");
        let images = ctx.docker.images();
        let opts = BuildOpts::builder(&ctx.image.path)
            .tag(&format!("{}:{}", &ctx.target.image(), LATEST))
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
                        &ctx.target,
                        LATEST,
                        &SystemTime::now(),
                        &ctx.docker,
                        &Default::default(),
                        ctx.simple,
                    )
                    .await?;

                    if let Ok(mut image_state) = ctx.image_state.write() {
                        (*image_state).update(&ctx.target, &state)
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
        let opts = BuildOpts::builder(&temp_path).tag(tag).build();

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
                        &ctx.build_ctx.target,
                        CACHED,
                        &SystemTime::now(),
                        &docker,
                        deps,
                        ctx.build_ctx.simple,
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
    image: &Path,
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
            if let Ok(entries) = fs::read_dir(image) {
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
