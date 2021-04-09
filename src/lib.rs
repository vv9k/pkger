#[macro_use]
extern crate anyhow;

mod cmd;
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
use log::{error, trace};
use moby::Docker;
use serde::Deserialize;
use std::cell::RefCell;
use std::convert::TryFrom;
use std::env;
use std::fs;
use std::path::Path;
use toml;
use tracing_subscriber;

pub const DEFAULT_CONF_FILE: &str = "conf.toml";
const DEFAULT_STATE_FILE: &str = ".pkger.state";

#[macro_export]
macro_rules! map_return {
    ($f:expr, $e:expr) => {
        match $f {
            Ok(d) => d,
            Err(e) => return Err(anyhow!("{} - {}", $e, e)),
        }
    };
}

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

pub struct Pkger {
    pub config: Config,
    images: Images,
    recipes: Recipes,
    docker: Docker,
    verbose: bool,
    images_state: RefCell<ImagesState>,
}

impl TryFrom<Config> for Pkger {
    type Error = Error;
    fn try_from(config: Config) -> Result<Self> {
        let images = Images::new(config.images_dir.clone())?;
        let recipes = Recipes::new(config.recipes_dir.clone())?;
        Ok(Pkger {
            config,
            images,
            recipes,
            docker: Docker::tcp("127.0.0.1:80"),
            verbose: true,
            images_state: RefCell::new(
                ImagesState::try_from_path(DEFAULT_STATE_FILE).unwrap_or_default(),
            ),
        })
    }
}

impl Pkger {
    async fn build_recipes(&self) {
        let mut tasks = Vec::new();
        for (_, recipe) in self.recipes.as_ref() {
            for image in &recipe.metadata.images {
                if let Some(image) = self.images.images().get(image) {
                    tasks.push(
                        JobRunner::new(BuildCtx::new(
                            &self.config,
                            &image,
                            &recipe,
                            &self.docker,
                            &self.images_state,
                            self.verbose,
                        ))
                        .run(),
                    );
                }
            }
        }

        for task in tasks {
            if let Err(e) = task.await {
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
        if let Err(e) = self.images_state.borrow().save() {
            error!("failed to save image state - {}", e);
        }
    }
    pub async fn main() -> Result<()> {
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

        dbg!(&config_path);

        let config = Config::from_path(&config_path)
            .map_err(|e| anyhow!("Failed to read config file from {} - {}", config_path, e))?;
        let mut pkger = Pkger::try_from(config)
            .map_err(|e| anyhow!("Failed to initialize pkger from config - {}", e))?;

        if !opts.recipes.is_empty() {
            let filtered = pkger
                .recipes
                .as_ref()
                .iter()
                .filter(|(recipe, _)| !&opts.recipes.contains(recipe))
                .map(|(recipe, _)| recipe.clone())
                .collect::<Vec<_>>();

            let recipes = pkger.recipes.as_ref_mut();
            for recipe in filtered {
                recipes.remove(&recipe);
            }
        }

        pkger.docker = docker_from_uri(opts.docker)
            .map_err(|e| anyhow!("Failed to initialize docker connection - {}", e))?;
        pkger.verbose = !opts.quiet;

        pkger.build_recipes().await;

        Ok(())
    }
}

fn docker_from_uri<U: AsRef<str>>(uri: Option<U>) -> Result<Docker> {
    match uri {
        Some(uri) => Docker::new(uri).map_err(|e| anyhow!("{}", e)),
        None => Ok(Docker::tcp("127.0.0.1:80")),
    }
}
