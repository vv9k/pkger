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
use std::cell::RefCell;
use std::convert::TryFrom;
use std::env;
use std::fs;
use std::path::Path;
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
    config: Config,
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
    fn process_opts(&mut self, opts: Opts) -> Result<()> {
        if !opts.recipes.is_empty() {
            let filtered = self
                .recipes
                .as_ref()
                .iter()
                .filter(|(recipe, _)| !&opts.recipes.contains(recipe))
                .map(|(recipe, _)| recipe.clone())
                .collect::<Vec<_>>();

            let recipes = self.recipes.as_ref_mut();
            for recipe in filtered {
                recipes.remove(&recipe);
            }
        }

        self.docker = match opts.docker {
            Some(uri) => Docker::new(uri).map_err(|e| anyhow!("{}", e)),
            None => Ok(Docker::tcp("127.0.0.1:80")),
        }
        .map_err(|e| anyhow!("Failed to initialize docker connection - {}", e))?;
        self.verbose = !opts.quiet;
        Ok(())
    }
    async fn build_recipes(&self) {
        let mut tasks = Vec::new();
        for (_, recipe) in self.recipes.as_ref() {
            for image_info in &recipe.metadata.images {
                if let Some(name) = image_info.get("name") {
                    if let Some(image) =
                        self.images.images().get(name.to_string().trim_matches('"'))
                    {
                        tasks.push(
                            JobRunner::new(BuildCtx::new(
                                &self.config,
                                &image,
                                &recipe,
                                &self.docker,
                                &self.images_state,
                                image_info
                                    .get("target")
                                    .map(|s| s.to_string().trim_matches('"').to_string()),
                                self.verbose,
                            ))
                            .run(),
                        );
                    }
                } else {
                    warn!("image missing name `{:?}`", image_info);
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
    pkger.build_recipes().await;

    Ok(())
}
