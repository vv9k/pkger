#[macro_use]
extern crate failure;
extern crate tar;
mod image;
mod package;
mod recipe;
mod util;
mod worker;
use self::image::*;
use self::recipe::*;
use self::util::*;
use self::worker::*;
use chrono::prelude::Local;
use failure::Error;
use futures::future::join_all;
use hyper::{Body, Uri};
use log::*;
use rpm;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, DirBuilder, DirEntry, File};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::str;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};
use tar::Archive;
use wharf::api::Container;
use wharf::opts::{
    ContainerBuilderOpts, ExecOpts, ImageBuilderOpts, RmContainerOpts, UploadArchiveOpts,
};
use wharf::result::CmdOut;
use wharf::Docker;

const DEFAULT_STATE_FILE: &str = ".pkger.state";
const TEMPORARY_BUILD_DIR: &str = "/tmp";

pub type State = Arc<Mutex<ImageState>>;

#[macro_export]
macro_rules! map_return {
    ($f:expr, $e:expr) => {
        match $f {
            Ok(d) => d,
            Err(e) => return Err(format_err!("{} - {}", $e, e)),
        }
    };
}

#[derive(Deserialize, Debug)]
pub struct Config {
    images_dir: String,
    recipes_dir: String,
    output_dir: String,
}

#[derive(Debug)]
pub struct Pkger {
    docker: Docker,
    pub config: Config,
    images: Images,
    recipes: Recipes,
}
impl Pkger {
    pub fn new(docker_addr: &str, conf_file: &str) -> Result<Self, Error> {
        let content = map_return!(
            fs::read(&conf_file),
            format!("failed to read config file from {}", conf_file)
        );
        let config: Config = map_return!(toml::from_slice(&content), "failed to parse config file");
        trace!("{:?}", config);
        let images = Pkger::parse_images_dir(&config.images_dir)?;
        let recipes = Pkger::parse_recipes_dir(&config.recipes_dir)?;
        Ok(Pkger {
            docker: Docker::new(docker_addr)?,
            config,
            images,
            recipes,
        })
    }

    fn parse_images_dir(p: &str) -> Result<Images, Error> {
        trace!("parsing images dir - {}", p);
        let mut images = HashMap::new();
        if Path::new(&p).exists() {
            for _entry in map_return!(fs::read_dir(p), format!("failed to read images_dir {}", p)) {
                if let Ok(entry) = _entry {
                    if let Ok(ftype) = entry.file_type() {
                        if ftype.is_dir() {
                            let image = Image::new(entry);
                            trace!("{:?}", image);
                            if image.has_dockerfile {
                                images.insert(image.name.clone(), image);
                            } else {
                                error!(
                                    "image {} doesn't have Dockerfile in it's root directory",
                                    image.name
                                );
                            }
                        }
                    }
                }
            }
        } else {
            warn!("images directory in {} doesn't exist", &p);
            info!("creating directory {}", &p);
            map_return!(
                fs::create_dir_all(&p),
                format!("failed to create directory for images in {}", &p)
            );
        }
        trace!("{:?}", images);
        Ok(images)
    }

    fn parse_recipes_dir(p: &str) -> Result<Recipes, Error> {
        trace!("parsing recipes dir - {}", p);
        let mut recipes = HashMap::new();
        if Path::new(&p).exists() {
            for _entry in map_return!(fs::read_dir(p), "failed to read recipes_dir") {
                if let Ok(entry) = _entry {
                    if let Ok(ftype) = entry.file_type() {
                        if ftype.is_dir() {
                            let path = entry.path();
                            match Recipe::new(entry) {
                                Ok(recipe) => {
                                    trace!("{:?}", recipe);
                                    recipes.insert(recipe.info.name.clone(), recipe);
                                }
                                Err(e) => error!(
                                    "directory {} doesn't have a recipe.toml or the recipe is wrong - {}",
                                    path.as_path().display(),
                                    e
                                ),
                            }
                        }
                    }
                }
            }
        } else {
            warn!("recipes directory in {} doesn't exist", &p);
            info!("creating directory {}", &p);
            map_return!(
                fs::create_dir_all(&p),
                format!("failed to create directory for recipes in {}", &p)
            );
        }
        trace!("{:?}", recipes);
        Ok(recipes)
    }

    pub async fn build_recipe<S: AsRef<str>>(&self, recipe: S) -> Result<(), Error> {
        let start = Instant::now();
        let mut names = Vec::new();
        let mut futures = Vec::new();
        match self.recipes.get(recipe.as_ref()) {
            Some(r) => {
                trace!("building recipe {:#?}", &r);
                let state = Arc::new(Mutex::new(
                    ImageState::load(DEFAULT_STATE_FILE).unwrap_or_default(),
                ));
                for image_name in r.info.images.iter() {
                    let image = match self.images.get(image_name) {
                        Some(i) => i,
                        None => {
                            error!(
                                "image {} not found in {}",
                                image_name, &self.config.images_dir
                            );
                            continue;
                        }
                    };
                    trace!("using image - {}", image_name);
                    names.push(image_name);
                    futures.push(Worker::spawn_working(
                        &self.config,
                        &self.docker,
                        &image,
                        &r,
                        Arc::clone(&state),
                    ));
                }
            }
            None => error!(
                "no recipe named {} found in recipes directory {}",
                recipe.as_ref(),
                self.config.recipes_dir
            ),
        }

        let f = join_all(futures).await;
        info!("Finished bulding recipe {}", recipe.as_ref());
        info!("Total build time: {} seconds", start.elapsed().as_secs());
        let results = names.iter().zip(f);
        let mut ok = Vec::new();
        let mut err = Vec::new();

        for result in results {
            match result.1 {
                Ok(_) => ok.push(result.0),
                Err(e) => err.push((result.0, e)),
            }
        }

        info!("Succesful builds:");
        ok.iter().for_each(|name| info!(" - {}", name));

        info!("Failed builds:");
        err.iter().for_each(|(name, e)| {
            error!(" - {}", name);
            error!("   - Error message: {}", e);
        });

        Ok(())
    }
}
