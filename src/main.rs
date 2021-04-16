#[macro_use]
extern crate anyhow;

mod cmd;
mod deps;
mod docker;
mod image;
mod job;
mod opts;
mod os;
mod recipe;
mod util;

use crate::docker::DockerConnectionPool;
use crate::image::{Images, ImagesState};
use crate::job::{BuildCtx, JobCtx, JobResult};
use crate::opts::Opts;
use crate::recipe::Recipes;

pub use anyhow::{Error, Result};
use serde::Deserialize;
use std::convert::TryFrom;
use std::env;
use std::fs;
use std::path::Path;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tokio::task;
use tracing::{debug, error, info, info_span, trace, warn, Level};
use tracing_subscriber::fmt::format;
use tracing_subscriber::prelude::*;

const DEFAULT_CONF_FILE: &str = "conf.toml";
const DEFAULT_STATE_FILE: &str = ".pkger.state";

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

impl TryFrom<Config> for Pkger {
    type Error = Error;
    fn try_from(config: Config) -> Result<Self> {
        let images = Images::new(config.images_dir.clone())?;
        let recipes = Recipes::new(config.recipes_dir.clone())?;
        Ok(Pkger {
            config: Arc::new(config),
            images: Arc::new(images),
            images_filter: Arc::new(vec![]),
            recipes: Arc::new(recipes),
            docker: Arc::new(DockerConnectionPool::default()),
            images_state: Arc::new(RwLock::new(
                ImagesState::try_from_path(DEFAULT_STATE_FILE).unwrap_or_default(),
            )),
            is_running: Arc::new(AtomicBool::new(true)),
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

        if let Some(images) = opts.images {
            if let Some(filter) = Arc::get_mut(&mut self.images_filter) {
                filter.extend(images);
            }
            trace!(images = ?self.images_filter, "building only on");
        }

        self.docker = Arc::new(
            // check if docker uri provided as cli arg
            match opts.docker {
                Some(uri) => DockerConnectionPool::new(uri),
                None => {
                    // otherwhise check if available as config parameter
                    if let Some(uri) = &self.config.docker {
                        DockerConnectionPool::new(uri)
                    } else {
                        Ok(DockerConnectionPool::default())
                    }
                }
            }
            .map_err(|e| anyhow!("Failed to initialize docker connection - {}", e))?,
        );
        Ok(())
    }

    async fn process_tasks(&self) {
        let mut tasks = Vec::new();
        for recipe in self.recipes.inner_ref().values() {
            for image_info in &recipe.metadata.images {
                if !self.images_filter.is_empty() && !self.images_filter.contains(&image_info.image)
                {
                    trace!(image = %image_info.image, "skipping");
                    continue;
                }
                if let Some(image) = self.images.images().get(&image_info.image) {
                    debug!(image = %image.name, recipe = %recipe.metadata.name, "spawning task");
                    tasks.push(task::spawn(
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
                error!(reason = %e, "failed to join the task");
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
}

fn setup_tracing_fmt(opts: &Opts) {
    let span = info_span!("setup-tracing");
    let _enter = span.enter();

    let filter = if let Some(filter) = env::var_os("RUST_LOG") {
        if opts.quiet {
            "".to_string()
        } else {
            filter.to_string_lossy().to_string()
        }
    } else if opts.quiet {
        "pkger=error".to_string()
    } else if opts.debug {
        "pkger=trace".to_string()
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

    let fmt = tracing_subscriber::fmt::fmt()
        .with_target(false)
        .with_level(true)
        .with_max_level(Level::TRACE)
        .with_env_filter(&filter)
        .fmt_fields(formatter)
        .event_format(format);

    if opts.hide_date {
        fmt.without_time().init()
    } else {
        fmt.with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc3339())
            .init()
    };

    trace!(log_filter = %filter);
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::from_args();
    trace!(opts = ?opts);

    setup_tracing_fmt(&opts);

    let config_path = opts
        .config
        .clone()
        .unwrap_or_else(|| DEFAULT_CONF_FILE.to_string());
    trace!(config_path = %config_path);

    let result = Config::from_path(&config_path);
    if let Err(e) = &result {
        error!(reason = %e, config_path = %config_path, "failed to read config file");
        process::exit(1);
    }
    let config = result.unwrap();
    trace!(config = ?config);

    let result = Pkger::try_from(config);
    if let Err(e) = &result {
        error!(reason = %e, "failed to initialize pkger from config");
        process::exit(1);
    }
    let mut pkger = result.unwrap();

    if let Err(e) = pkger.process_opts(opts) {
        error!(reason = %e, "failed to process opts");
        process::exit(1);
    }

    let is_running = pkger.is_running.clone();

    if let Err(e) = ctrlc::set_handler(move || {
        warn!("got ctrl-c");
        is_running.store(false, Ordering::SeqCst);
    }) {
        error!(reason = %e, "failed to set ctrl-c handler");
    };

    pkger.process_tasks().await;
    pkger.save_images_state();

    Ok(())
}
