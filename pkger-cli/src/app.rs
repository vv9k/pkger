use crate::config::Configuration;
use crate::gen;
use crate::job::{JobCtx, JobResult};
use crate::opts::{BuildOpts, Command, ListObject, Opts};
use pkger_core::build::Context;
use pkger_core::docker::DockerConnectionPool;
use pkger_core::gpg::GpgKey;
use pkger_core::image::{state::DEFAULT_STATE_FILE, Image, ImagesState};
use pkger_core::recipe::{self, BuildTarget, ImageTarget, Recipe};
use pkger_core::{ErrContext, Error, Result};

use async_rwlock::RwLock;
use futures::stream::FuturesUnordered;
use std::convert::TryFrom;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempdir::TempDir;
use tokio::task;
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
    let app_dir = tempdir.path();
    let images_dir = app_dir.join("images");
    if !images_dir.exists() {
        fs::create_dir_all(&images_dir)?;
    }

    Ok(tempdir)
}

pub struct Application {
    config: Arc<Configuration>,
    recipes: Arc<recipe::Loader>,
    docker: Arc<DockerConnectionPool>,
    images_state: Arc<RwLock<ImagesState>>,
    user_images_dir: PathBuf,
    is_running: Arc<AtomicBool>,
    app_dir: TempDir,
    gpg_key: Option<GpgKey>,
}

#[derive(Debug, PartialEq)]
pub enum BuildTask {
    Simple {
        recipe: Arc<Recipe>,
        target: BuildTarget,
    },
    Custom {
        recipe: Arc<Recipe>,
        target: ImageTarget,
    },
}

impl Application {
    pub fn new(config: Configuration) -> Result<Self> {
        let app_dir = create_app_dirs()?;
        let recipes = recipe::Loader::new(&config.recipes_dir)
            .context("failed to initialize recipe loader")?;
        let user_images_dir = config
            .images_dir
            .clone()
            .unwrap_or_else(|| app_dir.path().join("images"));

        let state_path = match dirs::cache_dir() {
            Some(dir) => dir.join(DEFAULT_STATE_FILE),
            None => PathBuf::from(DEFAULT_STATE_FILE),
        };

        let images_state = Arc::new(RwLock::new(
            match ImagesState::try_from_path(&state_path).context("failed to load images state") {
                Ok(state) => state,
                Err(e) => {
                    let e = format!("{:?}", e);
                    warn!(msg = %e);
                    ImagesState::new(&state_path)
                }
            },
        ));

        trace!(?images_state);

        let app = Application {
            config: Arc::new(config),
            recipes: Arc::new(recipes),
            docker: Arc::new(DockerConnectionPool::default()),
            images_state,
            user_images_dir,
            is_running: Arc::new(AtomicBool::new(true)),
            app_dir,
            gpg_key: None,
        };
        let is_running = app.is_running.clone();
        set_ctrlc_handler(is_running);
        Ok(app)
    }

    pub async fn process_opts(&mut self, opts: Opts) -> Result<()> {
        match opts.command {
            Command::Build(build_opts) => {
                if !build_opts.no_sign {
                    self.gpg_key = load_gpg_key(&self.config)?;
                }
                let tasks = self
                    .process_build_opts(build_opts)
                    .context("processing build opts")?;
                self.process_tasks(tasks, opts.quiet).await?;
                Ok(())
            }
            Command::GenRecipe(gen_recipe_opts) => gen::recipe(gen_recipe_opts),
            Command::List { object } => match object {
                ListObject::Images => self.list_images(),
                ListObject::Recipes => self.list_recipes(),
                ListObject::Packages { images } => self.list_packages(images),
            },
            Command::CleanCache => self.clean_cache().await,
            Command::Init { .. } => unreachable!(),
        }
    }

    async fn clean_cache(&mut self) -> Result<()> {
        let span = info_span!("clean-cache");
        let _entered = span.enter();

        let mut state = self.images_state.write().await;

        span.in_scope(|| {
            state.clear();
            state.save()
        })?;

        info!("ok");
        Ok(())
    }

    fn list_recipes(&self) -> Result<()> {
        for name in self.recipes.list()? {
            println!("{}", name);
        }
        Ok(())
    }

    fn list_packages(&self, images_filter: Option<Vec<String>>) -> Result<()> {
        let images = fs::read_dir(&self.config.output_dir)?.filter_map(|e| match e {
            Ok(e) => Some(e.path()),
            Err(e) => {
                warn!(reason = %format!("{:?}", e), "invalid entry");
                None
            }
        });

        let images: Vec<_> = if let Some(filter) = images_filter {
            images
                .filter(|image| {
                    filter.contains(
                        &image
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                    )
                })
                .collect()
        } else {
            images.collect()
        };

        for image in images {
            let image_name = image
                .file_name()
                .unwrap_or_else(|| image.as_os_str())
                .to_string_lossy();
            println!("{}:", image_name);
            match fs::read_dir(&image) {
                Ok(packages) => {
                    for package in packages {
                        match package {
                            Ok(package) => {
                                println!("\t{}", package.file_name().to_string_lossy());
                            }
                            Err(e) => {
                                error!(reason = %format!("{:?}", e), image = %image_name, "failed to list a package");
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(reason = %format!("{:?}", e), image = %image_name, "failed to list packages");
                }
            }
        }

        Ok(())
    }

    fn list_images(&self) -> Result<()> {
        fs::read_dir(&self.config.output_dir)?.for_each(|e| match e {
            Ok(e) => {
                println!("{}", e.file_name().to_string_lossy());
            }
            Err(e) => {
                warn!(reason = %format!("{:?}", e), "invalid entry");
            }
        });

        Ok(())
    }

    fn process_build_opts(&mut self, opts: BuildOpts) -> Result<Vec<BuildTask>> {
        let span = info_span!("process-build-opts");
        let _enter = span.enter();
        let mut tasks = Vec::new();
        let mut recipes = Vec::new();

        if opts.all {
            recipes = self
                .recipes
                .load_all()
                .context("loading recipes")?
                .into_iter()
                .map(Arc::new)
                .collect();
        } else if !opts.recipes.is_empty() {
            for recipe_name in opts.recipes {
                trace!(recipe = %recipe_name, "loading");
                recipes.push(Arc::new(
                    self.recipes.load(&recipe_name).context("loading recipe")?,
                ));
            }
        } else {
            warn!("no recipes to build");
            warn!("if you meant to build all recipes run `pkger build --all`");
            warn!("or only specified recipes with `pkger build <RECIPES>...`");
            return Ok(tasks);
        }

        macro_rules! add_task_if_target_found {
            ($target:ident, $recipe:ident, $self:ident, $tasks:ident) => {
                if let Some(target) = $self
                    .config
                    .images
                    .iter()
                    .find(|target| &target.image == $target)
                {
                    $tasks.push(BuildTask::Custom {
                        recipe: $recipe.clone(),
                        target: target.clone(),
                    });
                } else {
                    warn!(image = %$target, "not found in configuration");
                }
            };
        }

        if opts.all {
            debug!("building all recipes for all targets");
            for recipe in &recipes {
                if recipe.metadata.all_images {
                    for image in &self.config.images {
                        tasks.push(BuildTask::Custom {
                            target: image.clone(),
                            recipe: recipe.clone(),
                        });
                    }
                } else if let Some(images) = &recipe.metadata.images {
                    for target_image in images {
                        add_task_if_target_found!(target_image, recipe, self, tasks);
                    }
                } else {
                    warn!(recipe = %recipe.metadata.name, "recipe has no image targets, skipping");
                }
            }
        } else if let Some(targets) = &opts.simple {
            debug!("building only specified recipes for simple targets");
            for target in targets {
                for recipe in &recipes {
                    let target = BuildTarget::try_from(target.as_str())?;
                    tasks.push(BuildTask::Simple {
                        recipe: recipe.clone(),
                        target,
                    })
                }
            }
        } else if let Some(opt_images) = &opts.images {
            debug!("building only specified recipes for specified images");
            for recipe in &recipes {
                if recipe.metadata.all_images {
                    for image in opt_images {
                        add_task_if_target_found!(image, recipe, self, tasks);
                    }
                } else if let Some(images) = &recipe.metadata.images {
                    for image in opt_images {
                        // first we check if the recipe contains the image
                        if images.iter().any(|target| target == image) {
                            // then we fetch the target from configuration images
                            add_task_if_target_found!(image, recipe, self, tasks);
                        } else {
                            warn!(recipe = %recipe.metadata.name, %image, "image not found in recipe targets");
                        }
                    }
                } else {
                    warn!(recipe = %recipe.metadata.name, "recipe has no image targets, skipping");
                }
            }
        } else {
            trace!("building only specified recipes for all targets");
            for recipe in &recipes {
                if recipe.metadata.all_images {
                    for image in &self.config.images {
                        tasks.push(BuildTask::Custom {
                            target: image.clone(),
                            recipe: recipe.clone(),
                        });
                    }
                } else if let Some(images) = &recipe.metadata.images {
                    if images.is_empty() {
                        warn!(recipe = %recipe.metadata.name, "recipe has no image targets, skipping");
                        continue;
                    }
                    for target_image in images {
                        add_task_if_target_found!(target_image, recipe, self, tasks);
                    }
                } else {
                    warn!(recipe = %recipe.metadata.name, "recipe has no image targets, skipping");
                }
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
                    // otherwise check if available as config parameter
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
        Ok(tasks)
    }

    async fn process_tasks(&mut self, tasks: Vec<BuildTask>, quiet: bool) -> Result<()> {
        let span = info_span!("process-jobs");
        async move {
            let jobs = FuturesUnordered::new();
            for task in tasks {
                let (recipe, image, target, is_simple) =  match task {
                    BuildTask::Custom { recipe, target } => {
                        let image = Image::new(target.image.clone(), self.user_images_dir.join(&target.image));
                        (recipe, image, target, false)
                    }
                    BuildTask::Simple { recipe, target } => {
                        let image = Image::try_get_or_create(&self.app_dir.path().join("images"), target)?;
                        let name = image.name.clone();
                        (recipe, image, ImageTarget::new(name, target, None::<&str>), true)
                    }
                };
                    jobs.push(task::spawn(
                            JobCtx::Build(Context::new(
                                recipe,
                                image,
                                self.docker.connect(),
                                target,
                                self.config.output_dir.as_path(),
                                self.images_state.clone(),
                                self.is_running.clone(),
                                is_simple,
                                self.gpg_key.clone(),
                                self.config.ssh.clone(),
                                quiet
                            ))
                            .run(),
                        ));
                }

            let mut results = vec![];

            for job in jobs {
                let handle = job.await;
                if let Err(e) = handle {
                    error!(reason = %e, "failed to join the handle for a job");
                    continue;
                }
                results.push(handle.unwrap());
            }

            let mut task_failed = false;

            results.iter().for_each(|err| match err {
                JobResult::Failure { id, duration, reason } => {
                    task_failed = true;
                    error!(id = %id, reason = %reason, duration = %format!("{}s", duration.as_secs_f32()), "job failed");
                }
                JobResult::Success { id, duration, output } => {
                    info!(id = %id, output = %output, duration = %format!("{}s", duration.as_secs_f32()), "job succeded");
                }
            });

            if self.images_state.read().await.has_changed() {
                self.save_images_state().await;
            } else {
                trace!("images state unchanged, not saving");
            }

            if task_failed {
                Err(Error::msg("at least one of the tasks failed"))
            } else {
                Ok(())
            }
        }.instrument(span).await
    }

    async fn save_images_state(&self) {
        let span = info_span!("save-images-state");
        let _enter = span.enter();

        let state = self.images_state.read().await;

        if let Err(e) = state.save() {
            error!(reason = %format!("{:?}", e), "failed to save image state");
        }
    }
}

fn load_gpg_key(config: &Configuration) -> Result<Option<GpgKey>> {
    if let Some(key) = &config.gpg_key {
        let pass = rpassword::read_password_from_tty(Some("Gpg key password:"))
            .context("failed to read password for gpg key")?;
        if let Some(name) = &config.gpg_name {
            Ok(Some(GpgKey::new(key, name, &pass)?))
        } else {
            Err(Error::msg("missing `gpg_name` field from configuration"))
        }
    } else {
        Ok(None)
    }
}
