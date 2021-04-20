#[macro_use]
extern crate anyhow;

mod cmd;
mod container;
mod deps;
mod docker;
mod fmt;
mod image;
mod job;
mod opts;
mod os;
mod recipe;
mod util;

use crate::docker::DockerConnectionPool;
use crate::image::{Images, ImagesState};
use crate::job::{BuildCtx, JobCtx, JobResult};
use crate::opts::{BuildOpts, PkgerCmd, PkgerOpts};
use crate::recipe::Recipes;

pub use anyhow::{Error, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tokio::task;
use tracing::{debug, error, info, info_span, trace, warn, Instrument};

static DEFAULT_CONFIG_FILE: &str = ".pkger.toml";
static DEFAULT_STATE_FILE: &str = ".pkger.state";

#[derive(Deserialize, Debug)]
pub struct Config {
    images_dir: String,
    recipes_dir: String,
    output_dir: String,
    docker: Option<String>,
}
impl Config {
    fn from_path<P: AsRef<Path>>(val: P) -> Result<Self> {
        Ok(toml::from_slice(&fs::read(val.as_ref())?)?)
    }
}

struct Pkger {
    config: Arc<Config>,
    images: Arc<Images>,
    recipes: Arc<Recipes>,
    docker: Arc<DockerConnectionPool>,
    images_filter: Arc<Vec<String>>,
    images_state: Arc<RwLock<ImagesState>>,
    is_running: Arc<AtomicBool>,
}

impl From<Config> for Pkger {
    fn from(config: Config) -> Self {
        let images = Images::new(config.images_dir.as_str());
        let recipes = Recipes::new(config.recipes_dir.as_str());
        let pkger = Pkger {
            config: Arc::new(config),
            images: Arc::new(images),
            images_filter: Arc::new(vec![]),
            recipes: Arc::new(recipes),
            docker: Arc::new(DockerConnectionPool::default()),
            images_state: Arc::new(RwLock::new(
                ImagesState::try_from_path(DEFAULT_STATE_FILE).unwrap_or_default(),
            )),
            is_running: Arc::new(AtomicBool::new(true)),
        };
        let is_running = pkger.is_running.clone();
        set_ctrlc_handler(is_running);
        pkger
    }
}

impl Pkger {
    async fn process_opts(&mut self, opts: &PkgerOpts) -> Result<()> {
        match &opts.command {
            PkgerCmd::Build(build_opts) => {
                self.load_images();
                self.load_recipes();
                self.process_build_opts(&build_opts)?;
                self.process_tasks().await;
                self.save_images_state();
                Ok(())
            }
        }
    }
    fn process_build_opts(&mut self, opts: &BuildOpts) -> Result<()> {
        let span = info_span!("process-build-opts");
        let _enter = span.enter();

        if !opts.recipes.is_empty() {
            trace!(opts_recipes = %opts.recipes.join(", "));

            if let Some(recipes) = Arc::get_mut(&mut self.recipes) {
                let mut new_recipes = HashMap::new();
                let recipes = recipes.inner_ref_mut();
                for recipe_name in &opts.recipes {
                    if recipes.get(recipe_name).is_some() {
                        let recipe = recipes.remove(recipe_name).unwrap();
                        new_recipes.insert(recipe_name.to_string(), recipe);
                    } else {
                        warn!(recipe = %recipe_name, "not found in recipes");
                        continue;
                    }
                }
                *recipes = new_recipes;
            }

            if self.recipes.inner_ref().is_empty() {
                warn!("no recipes to build");
                return Ok(());
            }

            info!(recipes = ?self.recipes.inner_ref().keys().collect::<Vec<_>>(), "building only");
        } else {
            info!("building all recipes");
        }

        if let Some(images) = &opts.images {
            trace!(opts_images = ?images);
            if let Some(filter) = Arc::get_mut(&mut self.images_filter) {
                for image in images {
                    if self.images.images().get(image).is_none() {
                        warn!(image = %image, "not found in images");
                    } else {
                        filter.push(image.clone());
                    }
                }

                if self.images_filter.is_empty() {
                    warn!(
                        "image filter was provided but no provided images matched existing images"
                    );
                } else {
                    info!(images = ?self.images_filter, "building only on");
                }
            } else {
                info!("building on all images");
            }
        }

        self.docker = Arc::new(
            // check if docker uri provided as cli arg
            match &opts.docker {
                Some(uri) => {
                    trace!(uri = %uri, "using docker uri from opts");
                    DockerConnectionPool::new(uri)
                }
                None => {
                    // otherwhise check if available as config parameter
                    if let Some(uri) = &self.config.docker {
                        trace!(uri = %uri, "using docker uri from config");
                        DockerConnectionPool::new(uri)
                    } else {
                        trace!("using default docker uri");
                        Ok(DockerConnectionPool::default())
                    }
                }
            }
            .map_err(|e| anyhow!("Failed to initialize docker connection - {}", e))?,
        );
        Ok(())
    }

    async fn process_tasks(&self) {
        let span = info_span!("process-tasks");
        async move {
            let mut tasks = Vec::new();
            for recipe in self.recipes.inner_ref().values() {
                for image_info in &recipe.metadata.images {
                    if !self.images_filter.is_empty() && !self.images_filter.contains(&image_info.image)
                    {
                        debug!(image = %image_info.image, "skipping");
                        continue;
                    }
                    if let Some(image) = self.images.images().get(&image_info.image) {
                        debug!(image = %image.name, recipe = %recipe.metadata.name, "spawning task");
                        tasks.push(
                            task::spawn(
                                JobCtx::Build(BuildCtx::new(
                                    recipe.clone(),
                                    (*image).clone(),
                                    self.docker.connect(),
                                    image_info.target.clone(),
                                    self.config.clone(),
                                    self.images_state.clone(),
                                    self.is_running.clone(),
                                ))
                                .run(),
                            )
                        );
                    } else {
                        warn!(image = %image_info.image, "not found");
                    }
                }
            }

            let mut errors = vec![];

            for task in tasks {
                let handle = task.await;
                if let Err(e) = handle {
                    error!(reason = %e, "failed to join the task");
                    continue;
                }

                errors.push(handle.unwrap());
            }

            errors.iter().for_each(|err| match err {
                JobResult::Failure { id, reason } => {
                    error!(id = %id, reason = %reason, "job failed");
                }
                JobResult::Success { id, output } => {
                    info!(id = %id, output = %output, "job succeded");
                }
            });
        }.instrument(span).await
    }

    fn save_images_state(&self) {
        let span = info_span!("save-images-state");
        let _enter = span.enter();

        let result = self.images_state.read();

        if let Err(e) = result {
            error!(reason = %e, "failed to save image state");
            return;
        }

        // it's ok to unwrap, we check the wrapping error above
        if let Err(e) = (*result.unwrap()).save() {
            error!(reason = %e, "failed to save image state");
        }
    }

    fn load_images(&mut self) {
        if let Some(images) = Arc::get_mut(&mut self.images) {
            if let Err(e) = images.load() {
                error!(
                    reason = %e,
                    "failed to load images"
                );
                process::exit(1);
            }
        } else {
            error!(
                reason = "couldn't get mutable reference to images",
                "failed to load images"
            );
            process::exit(1);
        }
    }

    fn load_recipes(&mut self) {
        if let Some(recipes) = Arc::get_mut(&mut self.recipes) {
            if let Err(e) = recipes.load() {
                error!(
                    reason = %e,
                    "failed to load recipes"
                );
                process::exit(1);
            }
        } else {
            error!(
                reason = "couldn't get mutable reference to recipes",
                "failed to load recipes"
            );
            process::exit(1);
        }
    }
}

fn set_ctrlc_handler(is_running: Arc<AtomicBool>) {
    if let Err(e) = ctrlc::set_handler(move || {
        warn!("got ctrl-c");
        is_running.store(false, Ordering::SeqCst);
    }) {
        error!(reason = %e, "failed to set ctrl-c handler");
    };
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = PkgerOpts::from_args();

    fmt::setup_tracing(&opts);

    trace!(opts = ?opts);

    // config
    let config_path = opts.config.clone().unwrap_or_else(|| {
        match dirs_next::home_dir() {
            Some(home_dir) => {
                home_dir.join(DEFAULT_CONFIG_FILE).to_string_lossy().to_string()
            }
            None => {
                warn!(path = %DEFAULT_CONFIG_FILE, "current user has no home directory, using default");
                DEFAULT_CONFIG_FILE.to_string()
            }
        }
    });
    trace!(config_path = %config_path);
    let result = Config::from_path(&config_path);
    if let Err(e) = &result {
        error!(reason = %e, config_path = %config_path, "failed to read config file");
        process::exit(1);
    }
    let config = result.unwrap();
    trace!(config = ?config);

    let mut pkger = Pkger::from(config);
    if let Err(e) = pkger.process_opts(&opts).await {
        error!(reason = %e, "execution failed");
        process::exit(1);
    }
    Ok(())
}
