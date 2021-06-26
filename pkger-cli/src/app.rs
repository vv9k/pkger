use crate::config::Configuration;
use crate::gen;
use crate::job::{JobCtx, JobResult};
use crate::opts::{BuildOpts, Commands, ListObject, Opts};
use pkger_core::build::Context;
use pkger_core::docker::DockerConnectionPool;
use pkger_core::image::{Image, Images, ImagesState, DEFAULT_STATE_FILE};
use pkger_core::recipe::{BuildTarget, ImageTarget, Recipes};
use pkger_core::{ErrContext, Error, Result};

use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tempdir::TempDir;
use tokio::task;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, info_span, trace, warn, Instrument};

fn set_ctrlc_handler(is_running: Arc<AtomicBool>) {
    if let Err(e) = ctrlc::set_handler(move || {
        warn!("got ctrl-c");
        is_running.store(false, Ordering::SeqCst);
    }) {
        error!(reason = %e, "failed to set ctrl-c handler");
    };
}

fn create_app_dirs() -> Result<TempDir> {
    let tempdir = TempDir::new("pkger")?;
    let pkger_dir = tempdir.path();
    let images_dir = pkger_dir.join("images");
    if !images_dir.exists() {
        fs::create_dir_all(&images_dir)?;
    }

    Ok(tempdir)
}

pub struct Application {
    config: Arc<Configuration>,
    user_images: Arc<Option<Images>>,
    recipes: Arc<Recipes>,
    docker: Arc<DockerConnectionPool>,
    images_filter: Arc<Vec<String>>,
    images_state: Arc<RwLock<ImagesState>>,
    is_running: Arc<AtomicBool>,
    simple_targets: Vec<String>,
    _pkger_dir: TempDir,
}

impl Application {
    pub fn new(config: Configuration) -> Result<Self> {
        let _pkger_dir = create_app_dirs()?;
        let user_images = config.images_dir.as_ref().map(|path| Images::new(&path));
        let recipes = Recipes::new(&config.recipes_dir);
        let pkger = Application {
            config: Arc::new(config),
            user_images: Arc::new(user_images),
            images_filter: Arc::new(vec![]),
            recipes: Arc::new(recipes),
            docker: Arc::new(DockerConnectionPool::default()),
            images_state: Arc::new(RwLock::new(
                ImagesState::try_from_path(DEFAULT_STATE_FILE).unwrap_or_default(),
            )),
            is_running: Arc::new(AtomicBool::new(true)),
            simple_targets: vec![],
            _pkger_dir,
        };
        let is_running = pkger.is_running.clone();
        set_ctrlc_handler(is_running);
        Ok(pkger)
    }

    pub async fn process_opts(&mut self, opts: Opts) -> Result<()> {
        match opts.command {
            Commands::Build(build_opts) => {
                self.load_user_images()?;
                self.load_recipes()?;
                self.process_build_opts(&build_opts)?;
                self.process_tasks().await?;
                self.save_images_state();
                Ok(())
            }
            Commands::GenRecipe(gen_recipe_opts) => gen::recipe(gen_recipe_opts),
            Commands::List(list_opts) => match list_opts.object {
                ListObject::Images => {
                    self.load_user_images()?;
                    self.list_images();
                    Ok(())
                }
                ListObject::Recipes => {
                    self.load_recipes()?;
                    self.list_recipes();
                    Ok(())
                }
            },
        }
    }

    fn list_recipes(&self) {
        for name in self.recipes.inner_ref().keys() {
            println!("{}", name);
        }
    }

    fn list_images(&self) {
        if let Some(images) = Option::as_ref(&self.user_images) {
            for image in images.images().keys() {
                println!("{}", image);
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

        if let Some(targets) = &opts.simple {
            targets
                .iter()
                .for_each(|target| self.simple_targets.push(target.to_string()));
        } else if let Some(opt_images) = &opts.images {
            if let Some(user_images) = self.user_images.as_ref() {
                trace!(opts_images = ?opt_images);
                if let Some(filter) = Arc::get_mut(&mut self.images_filter) {
                    for image in opt_images {
                        if user_images.images().get(image).is_none() {
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
            } else {
                warn!("no custom images found, not building any recipes");
                return Ok(());
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
            .context("Failed to initialize docker connection")?,
        );
        Ok(())
    }

    async fn process_tasks(&mut self) -> Result<()> {
        let span = info_span!("process-tasks");
        async move {
            let mut tasks = Vec::new();

            if self.simple_targets.is_empty() {
                self.spawn_custom_image_tasks(&mut tasks);
            } else {
                trace!(targets = ?self.simple_targets, "building simple targets");
                let mut targets = Vec::new();
                let location = self._pkger_dir.path().join("images");
                for target in &self.simple_targets {
                    let target = BuildTarget::try_from(target.as_str())?;
                    let image = Image::new(&location, &target)?;
                    targets.push((image,target));
                }
                self.spawn_simple_tasks(&targets[..], &mut tasks);
            };

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
                JobResult::Failure { id, duration, reason } => {
                    error!(id = %id, reason = %reason, duration = %format!("{}s", duration.as_secs_f32()), "job failed");
                }
                JobResult::Success { id, duration, output } => {
                    info!(id = %id, output = %output, duration = %format!("{}s", duration.as_secs_f32()), "job succeded");
                }
            });

            Ok(())
        }.instrument(span).await
    }

    fn spawn_simple_tasks(
        &self,
        images: &[(Image, BuildTarget)],
        tasks: &mut Vec<JoinHandle<JobResult>>,
    ) {
        for recipe in self.recipes.inner_ref().values() {
            for (image, target) in images {
                debug!(image = %image.name, recipe = %recipe.metadata.name, "spawning task");
                tasks.push(task::spawn(
                    JobCtx::Build(Context::new(
                        recipe.clone(),
                        (*image).clone(),
                        self.docker.connect(),
                        ImageTarget::new(&image.name, &target, None::<&str>),
                        self.config.output_dir.as_path(),
                        self.images_state.clone(),
                        self.is_running.clone(),
                        true,
                    ))
                    .run(),
                ));
            }
        }
    }

    fn spawn_custom_image_tasks(&self, tasks: &mut Vec<JoinHandle<JobResult>>) {
        for recipe in self.recipes.inner_ref().values() {
            let recipe_images = if let Some(images) = &recipe.metadata.images {
                images
            } else {
                continue;
            };

            let mut names = recipe_images.iter().map(|it| &it.image);
            for image_name in self.images_filter.iter() {
                if !names.any(|name| name == image_name) {
                    warn!(recipe = %recipe.metadata.name, image = %image_name, "image specified as argument but missing from recipe");
                }
            }

            for it in recipe_images {
                if !self.images_filter.is_empty() && !self.images_filter.contains(&it.image) {
                    debug!(image = %it.image, "skipping");
                    continue;
                }
                debug!(image = %it.image, recipe = %recipe.metadata.name, "spawning task");
                let images = if let Some(images) = self.user_images.as_ref() {
                    images
                } else {
                    warn!("no custom images found, not building any recipes");
                    return;
                };

                if let Some(image) = images.images().get(&it.image) {
                    tasks.push(task::spawn(
                        JobCtx::Build(Context::new(
                            recipe.clone(),
                            (*image).clone(),
                            self.docker.connect(),
                            it.clone(),
                            self.config.output_dir.as_path(),
                            self.images_state.clone(),
                            self.is_running.clone(),
                            false,
                        ))
                        .run(),
                    ));
                } else {
                    warn!(image= %it.image, "not found");
                }
            }
        }
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

    fn load_user_images(&mut self) -> Result<()> {
        let images = if let Some(images) = Arc::get_mut(&mut self.user_images) {
            if let Some(images) = images {
                images
            } else {
                return Err(Error::msg("no user images to load"));
            }
        } else {
            return Err(Error::msg("failed to get mutable reference to user images"));
        };

        images.load().context("failed to load images")
    }

    fn load_recipes(&mut self) -> Result<()> {
        if let Some(recipes) = Arc::get_mut(&mut self.recipes) {
            recipes.load().context("failed to load recipes")
        } else {
            Err(Error::msg(
                "failed to load recipes - couldn't get mutable reference to recipes",
            ))
        }
    }
}
