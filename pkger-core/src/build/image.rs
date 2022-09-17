use crate::build::{container, deps, Context};
use crate::image::{ImageState, ImagesState};
use crate::log::{debug, info, trace, warning, BoxedCollector};
use crate::recipe::RecipeTarget;
use crate::runtime::RuntimeConnector;
use crate::{err, Error, Result};
use docker_api::models::ImageBuildChunk;
use docker_api::opts::ImageBuildOpts;

use async_rwlock::RwLock;
use futures::StreamExt;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tempdir::TempDir;

pub static CACHED: &str = "cached";
pub static LATEST: &str = "latest";

pub async fn build(ctx: &mut Context, logger: &mut BoxedCollector) -> Result<ImageState> {
    info!(logger => "building image '{}'", ctx.target.image());
    let mut deps = if let Some(deps) = &ctx.recipe.metadata.build_depends {
        deps.resolve_names(ctx.target.image())
    } else {
        Default::default()
    };
    deps.extend(deps::default(
        ctx.target.build_target(),
        &ctx.recipe,
        ctx.gpg_key.is_some(),
    ));
    trace!(logger => "resolved dependencies: {:?}", deps);

    let state = find_cached_state(
        &ctx.image.path,
        &ctx.target,
        &ctx.image_state,
        ctx.simple,
        logger,
    )
    .await;

    if let Some(state) = state {
        let state_deps = state
            .deps
            .iter()
            .map(|s| s.as_str())
            .collect::<HashSet<_>>();
        if deps != state_deps {
            info!(logger => "dependencies changed, old: {:?}, new: {:?}", state_deps, deps);
        } else {
            trace!(logger => "dependencies unchanged");

            if state.exists(&ctx.runtime, logger).await {
                trace!(logger => "image state exists in docker, reusing");
                return Ok(state);
            } else {
                warning!(logger => "found cached state but image doesn't exist in docker")
            }
        }
    }

    debug!(logger => "building from scratch");

    match &ctx.runtime {
        RuntimeConnector::Docker(docker) => {
            let images = docker.images();
            let opts = ImageBuildOpts::builder(&ctx.image.path)
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
                        return err!(error);
                    }
                    ImageBuildChunk::Update { stream } => {
                        info!(logger => "{}", stream);
                    }
                    ImageBuildChunk::Digest { aux } => {
                        let state = ImageState::new(
                            &aux.id,
                            &ctx.target,
                            LATEST,
                            &SystemTime::now(),
                            &ctx.runtime,
                            &Default::default(),
                            ctx.simple,
                            logger,
                        )
                        .await?;

                        trace!(logger => "updating image state {}", state.id);
                        let mut image_state = ctx.image_state.write().await;
                        (*image_state).update(ctx.target.clone(), state.clone());

                        return Ok(state);
                    }
                    _ => {}
                }
            }
        }
        RuntimeConnector::Podman(podman) => {
            use podman_api::opts::ImageBuildOpts;

            let opts = ImageBuildOpts::builder(ctx.image.path.to_string_lossy())
                .tag(format!("{}:{}", ctx.target.image(), LATEST))
                .build();

            let images = podman.images();

            let mut stream = images.build(&opts)?;

            let mut last = None;
            while let Some(chunk) = stream.next().await {
                last = Some(chunk?);

                info!(logger => "{}", last.as_ref().unwrap().stream);
            }

            let id = if let Some(last) = last {
                last.stream.trim_end().to_owned()
            } else {
                return err!("expected stream response for image build");
            };
            let state = ImageState::new(
                &id,
                &ctx.target,
                LATEST,
                &SystemTime::now(),
                &ctx.runtime,
                &Default::default(),
                ctx.simple,
                logger,
            )
            .await?;

            trace!(logger => "updating image state {}", state.id);
            let mut image_state = ctx.image_state.write().await;
            (*image_state).update(ctx.target.clone(), state.clone());

            return Ok(state);
        }
    };

    err!("stream ended before image id was received")
}

pub async fn create_cache(
    ctx: &container::Context<'_>,
    state: &ImageState,
    deps: &HashSet<&str>,
    logger: &mut BoxedCollector,
) -> Result<ImageState> {
    info!(logger => "caching image '{}'", state.image);
    let pkg_mngr = state.os.package_manager();
    let pkg_mngr_name = pkg_mngr.as_ref();
    let tag = format!("{}:{}", state.image, state.tag);

    if pkg_mngr_name.is_empty() {
        return err!(
            "caching image failed - no package manger found for os `{}`",
            state.os.name()
        );
    }

    let deps_joined = deps.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    #[rustfmt::skip]
            let dockerfile = format!(
r#"FROM {}
ENV DEBIAN_FRONTEND noninteractive
{}
RUN {} {} && \
    {} {} {}
"#,
                tag,
                if pkg_mngr.should_clean_cache() { format!("RUN {} {}", pkg_mngr_name, pkg_mngr.clean_cache().join(" "))} else { String::new() },
                pkg_mngr_name, pkg_mngr.update_repos_args().join(" "),
                pkg_mngr_name, pkg_mngr.install_args().join(" "), deps_joined.join(" ")
            );

    trace!(logger => "Dockerfile:\n{}", dockerfile);

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
    trace!(logger => "temp dir: {}", temp_path.display());
    fs::write(temp_path.join("Dockerfile"), dockerfile)?;

    match &ctx.build.runtime {
        RuntimeConnector::Docker(docker) => {
            let images = docker.images();
            let opts = ImageBuildOpts::builder(&temp_path)
                .tag(format!("{}:{}", state.image, CACHED))
                .build();

            let mut stream = images.build(&opts);

            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                match chunk {
                    ImageBuildChunk::Error {
                        error,
                        error_detail: _,
                    } => {
                        return err!(error);
                    }
                    ImageBuildChunk::Update { stream } => {
                        info!(logger => "{}", stream);
                    }
                    ImageBuildChunk::Digest { aux } => {
                        return ImageState::new(
                            &aux.id,
                            &ctx.build.target,
                            CACHED,
                            &SystemTime::now(),
                            &ctx.build.runtime,
                            deps,
                            ctx.build.simple,
                            logger,
                        )
                        .await
                    }
                    _ => {}
                }
            }

            err!("id of image not received")
        }
        RuntimeConnector::Podman(podman) => {
            use podman_api::opts::ImageBuildOpts;

            let opts = ImageBuildOpts::builder(temp_path.to_string_lossy())
                .tag(format!("{}:{}", state.image, CACHED))
                .build();

            let images = podman.images();

            let mut stream = images.build(&opts)?;

            let mut last = None;
            while let Some(chunk) = stream.next().await {
                last = Some(chunk?);

                info!(logger => "{}", last.as_ref().unwrap().stream);
            }

            let id = if let Some(last) = last {
                last.stream.trim_end().to_owned()
            } else {
                return err!("expected stream response for image build");
            };
            let state = ImageState::new(
                &id,
                &ctx.build.target,
                LATEST,
                &SystemTime::now(),
                &ctx.build.runtime,
                &Default::default(),
                ctx.build.simple,
                logger,
            )
            .await?;

            trace!(logger => "updating image state {}", state.id);
            let mut image_state = ctx.build.image_state.write().await;
            (*image_state).update(ctx.build.target.clone(), state.clone());

            Ok(state)
        }
    }
}

/// Checks whether any of the files located at the path of this Image changed since last build.
/// If shouldn't be rebuilt returns previous `ImageState`.
pub async fn find_cached_state(
    image: &Path,
    target: &RecipeTarget,
    state: &RwLock<ImagesState>,
    simple: bool,
    logger: &mut BoxedCollector,
) -> Option<ImageState> {
    info!(logger => "finding cache for image {}", image.display());

    trace!(logger => "{:?}", target);

    trace!("checking if image should be rebuilt");
    let states = state.read().await;
    if let Some(state) = (*states).images.get(target) {
        if simple {
            return Some(state.to_owned());
        }
        if let Ok(entries) = fs::read_dir(image) {
            for file in entries {
                if let Err(e) = file {
                    warning!(logger => "error while loading file, reason: {:?}", e);
                    continue;
                }
                let file = file.unwrap();
                let path = file.path();
                trace!(logger => "checking '{}'", path.display());
                let metadata = fs::metadata(path.as_path());
                if let Err(e) = metadata {
                    warning!(logger => "failed to read metadata, reason: {:?}", e);
                    continue;
                }
                let metadata = metadata.unwrap();
                let mod_time = metadata.modified();
                if let Err(e) = &mod_time {
                    warning!(logger => "failed to check modification time, reason: {:?}", e);
                    continue;
                }
                let mod_time = mod_time.unwrap();
                if mod_time > state.timestamp {
                    trace!(logger => "found modified file - not returning cache, mod time: {:?}, image mod time: {:?}", mod_time, state.timestamp);
                    return None;
                }
            }
        }
        let state = state.to_owned();
        trace!(logger => "found cached state: {:?}", state);
        return Some(state);
    }

    None
}
