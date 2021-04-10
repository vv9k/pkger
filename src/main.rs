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
use crate::job::{BuildCtx, JobRunner};
use crate::opts::Opts;
use crate::recipe::Recipes;

pub use anyhow::{Error, Result};
use log::{error, trace, warn};
use moby::Docker;
use serde::Deserialize;
use std::convert::TryFrom;
use std::env;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tokio::task;
use toml;
use tracing_subscriber;

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
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::from_args();
    if !opts.quiet {
        if env::var_os("RUST_LOG").is_none() {
            env::set_var("RUST_LOG", "pkger=info");
        }
        tracing_subscriber::fmt::init();
    }
    trace!("{:?}", opts);

    let config_path = opts
        .config
        .clone()
        .unwrap_or_else(|| DEFAULT_CONF_FILE.to_string());
    let config = Config::from_path(&config_path)
        .map_err(|e| anyhow!("Failed to read config file from {} - {}", config_path, e))?;

    let mut pkger = Pkger::try_from(config)
        .map_err(|e| anyhow!("Failed to initialize pkger from config - {}", e))?;
    pkger.process_opts(opts)?;
    let mut tasks = Vec::new();

    for (_, recipe) in pkger.recipes.inner_ref() {
        for image_info in &recipe.metadata.images {
            if let Some(image) = pkger.images.images().get(&image_info.image) {
                tasks.push(task::spawn(
                    JobRunner::new(BuildCtx::new(
                        recipe.clone(),
                        (*image).clone(),
                        pkger.config.clone(),
                        pkger.docker.clone(),
                        pkger.images_state.clone(),
                        image_info.target.clone(),
                        pkger.verbose,
                    ))
                    .run(),
                ));
            } else {
                warn!("image `{}` not found", &image_info.image);
            }
        }
    }

    for task in tasks {
        let handle = task.await;
        if let Err(e) = handle {
            error!("failed to join the task - {}", e);
            continue;
        }

        // it's ok to unwrap, we check the error above
        if let Err(e) = handle.unwrap() {
            let reason = match e.downcast::<moby::Error>() {
                Ok(err) => match err {
                    moby::Error::Fault { code: _, message } => message,
                    e => e.to_string(),
                },
                Err(e) => e.to_string(),
            };
            error!("job failed - {}", reason);
        }
    }
    let result = pkger.images_state.read();

    if let Err(e) = result {
        error!("failed to save image state - {}", e);
        return Ok(());
    }

    // it's ok to unwrap, we check the error above
    if let Err(e) = (*result.unwrap()).save() {
        error!("failed to save image state - {}", e);
    }

    Ok(())
}
