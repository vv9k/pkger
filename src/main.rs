#[macro_use]
extern crate anyhow;

mod cmd;
mod deps;
mod image;
mod job;
mod opts;
mod os;
mod package;
mod recipe;
mod util;

use crate::image::{Images, ImagesState};
use crate::job::{BuildCtx, JobResult, JobRunner};
use crate::opts::Opts;
use crate::recipe::Recipes;

pub use anyhow::{Error, Result};
use moby::Docker;
use serde::Deserialize;
use std::convert::TryFrom;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tokio::task;
use toml;
use tracing::{debug, error, info, trace, warn, Level};
use tracing_subscriber;
use tracing_subscriber::fmt::format;
use tracing_subscriber::prelude::*;

const DEFAULT_CONF_FILE: &str = "conf.toml";
const DEFAULT_STATE_FILE: &str = ".pkger.state";

#[derive(Deserialize, Debug)]
pub struct Config {
    images_dir: String,
    recipes_dir: String,
    output_dir: String,
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
    docker: Arc<Docker>,
    verbose: bool,
    images_state: Arc<RwLock<ImagesState>>,
}

impl TryFrom<Config> for Pkger {
    type Error = Error;
    fn try_from(config: Config) -> Result<Self> {
        let images = Images::new(config.images_dir.clone())?;
        let recipes = Recipes::new(config.recipes_dir.clone())?;
        Ok(Pkger {
            config: Arc::new(config),
            images: Arc::new(images),
            recipes: Arc::new(recipes),
            docker: Arc::new(Docker::tcp("127.0.0.1:80")),
            verbose: true,
            images_state: Arc::new(RwLock::new(
                ImagesState::try_from_path(DEFAULT_STATE_FILE).unwrap_or_default(),
            )),
        })
    }
}

impl Pkger {
    fn process_opts(&mut self, opts: Opts) -> Result<()> {
        if !opts.recipes.is_empty() {
            let filtered = self
                .recipes
                .inner_ref()
                .iter()
                .filter(|(recipe, _)| !&opts.recipes.contains(recipe))
                .map(|(recipe, _)| recipe.clone())
                .collect::<Vec<_>>();

            if let Some(recipes) = Arc::get_mut(&mut self.recipes) {
                let recipes = recipes.inner_ref_mut();
                for recipe in filtered {
                    recipes.remove(&recipe);
                }
            }
        }

        self.docker = Arc::new(
            match opts.docker {
                Some(uri) => Docker::new(uri).map_err(|e| anyhow!("{}", e)),
                None => Ok(Docker::tcp("127.0.0.1:80")),
            }
            .map_err(|e| anyhow!("Failed to initialize docker connection - {}", e))?,
        );
        self.verbose = !opts.quiet;
        Ok(())
    }

    async fn process_tasks(&self) {
        let mut tasks = Vec::new();
        for (_, recipe) in self.recipes.inner_ref() {
            for image_info in &recipe.metadata.images {
                if let Some(image) = self.images.images().get(&image_info.image) {
                    debug!(image = %image.name, recipe = %recipe.metadata.name, "spawning task");
                    tasks.push(task::spawn(
                        JobRunner::new(BuildCtx::new(
                            recipe.clone(),
                            (*image).clone(),
                            self.config.clone(),
                            self.docker.clone(),
                            self.images_state.clone(),
                            image_info.target.clone(),
                            self.verbose,
                        ))
                        .run(),
                    ));
                } else {
                    warn!(image = %image_info.image, "not found");
                }
            }
        }

        let mut errors = vec![];

        for task in tasks {
            let handle = task.await;
            if let Err(e) = handle {
                error!("failed to join the task - {}", e);
                continue;
            }

            errors.push(handle.unwrap());
        }

        errors.iter().for_each(|err| match err {
            JobResult::Failure { id, reason } => {
                error!(id = %id, reason = %reason, "job failed");
            }
            JobResult::Success { id } => {
                info!(id = %id, "job succeded");
            }
        });
    }

    fn save_images_state(&self) {
        let result = self.images_state.read();

        if let Err(e) = result {
            error!("failed to save image state - {}", e);
            return;
        }

        // it's ok to unwrap, we check the wrapping error above
        if let Err(e) = (*result.unwrap()).save() {
            error!("failed to save image state - {}", e);
        }
    }
}

fn setup_tracing_fmt() {
    let filter = if let Some(filter) = env::var_os("RUST_LOG") {
        filter.to_string_lossy().to_string()
    } else {
        "pkger=info".to_string()
    };

    let formatter =
            // Construct a custom formatter for `Debug` fields
            format::debug_fn(|writer, field, value| {
                if field.name() == "message" {
                    write!(writer, "{:?}",value)
                } else {
                    write!(writer, "{} = {:?}", field, value)
                }
            }).delimited(", ");

    let format = tracing_subscriber::fmt::format()
        .with_target(false)
        .with_level(true);

    tracing_subscriber::fmt::fmt()
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc3339())
        .with_level(true)
        .with_max_level(Level::TRACE)
        .with_env_filter(filter)
        .fmt_fields(formatter)
        .event_format(format)
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::from_args();
    if !opts.quiet {
        setup_tracing_fmt();
    }
    trace!(opts = ?opts);

    let config_path = opts
        .config
        .clone()
        .unwrap_or_else(|| DEFAULT_CONF_FILE.to_string());
    trace!(config_path = %config_path);

    let config = Config::from_path(&config_path)
        .map_err(|e| anyhow!("Failed to read config file from {} - {}", config_path, e))?;
    trace!(config = ?config);

    let mut pkger = Pkger::try_from(config)
        .map_err(|e| anyhow!("Failed to initialize pkger from config - {}", e))?;

    pkger.process_opts(opts)?;
    pkger.process_tasks().await;
    pkger.save_images_state();

    Ok(())
}
