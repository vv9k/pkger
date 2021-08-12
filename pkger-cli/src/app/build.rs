use crate::app::{Application, BuildTask};
use crate::job::{JobCtx, JobResult};
use crate::opts::BuildOpts;
use pkger_core::build::Context;
use pkger_core::docker::DockerConnectionPool;
use pkger_core::image::Image;
use pkger_core::recipe::{BuildTarget, ImageTarget};
use pkger_core::{err, ErrContext, Error, Result};

use futures::stream::FuturesUnordered;
use std::convert::TryFrom;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::task;
use tracing::{debug, error, info, info_span, trace, warn, Instrument};

impl Application {
    pub fn process_build_opts(&mut self, opts: BuildOpts) -> Result<Vec<BuildTask>> {
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

    pub async fn process_tasks(&mut self, tasks: Vec<BuildTask>, quiet: bool) -> Result<()> {
        let span = info_span!("process-jobs");
        async move {
            let jobs = FuturesUnordered::new();
            for task in tasks {
                let (recipe, image, target, is_simple) = match task {
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
                if !self.is_running.load(Ordering::Relaxed) {
                    warn!("got ctrl-c, killing job");
                    job.abort();
                    continue
                }
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
                err!("at least one of the tasks failed")
            } else {
                Ok(())
            }
        }.instrument(span).await
    }
}
