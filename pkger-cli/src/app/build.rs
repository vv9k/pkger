use crate::app::{AppOutputConfig, Application};
use crate::job::{JobCtx, JobResult};
use crate::opts::BuildOpts;
use pkger_core::build::{container::SESSION_LABEL_KEY, Context};
use pkger_core::image::Image;
use pkger_core::log::{self, debug, error, info, trace, warning, BoxedCollector};
use pkger_core::recipe::{BuildTarget, ImageTarget, Recipe};
use pkger_core::runtime::{self, RuntimeConnector};
use pkger_core::{err, ErrContext, Error, Result};

use futures::stream::FuturesUnordered;
use std::collections::{HashMap, VecDeque};
use std::convert::TryFrom;
use tokio::task;

#[derive(Debug, PartialEq)]
pub enum BuildTask {
    Simple {
        recipe: Recipe,
        target: BuildTarget,
        version: String,
    },
    Custom {
        recipe: Recipe,
        target: ImageTarget,
        version: String,
    },
}

impl Application {
    pub fn process_build_opts(
        &mut self,
        opts: BuildOpts,
        logger: &mut BoxedCollector,
    ) -> Result<Vec<BuildTask>> {
        debug!(logger => "processing build opts");

        let mut tasks = Vec::new();
        let mut recipes_to_build = Vec::new();

        if let Some(output_dir) = opts.output_dir {
            debug!(logger => "overriding output directory for this build, output_dir = {}", output_dir.display());
            self.config.output_dir = output_dir;
        }

        if opts.all {
            recipes_to_build = self
                .recipes
                .load_all(logger)
                .context("loading recipes")?
                .into_iter()
                .map(|r| {
                    let versions = r.metadata.version.versions().to_vec();
                    (r, versions)
                })
                .collect();
        } else if !opts.recipes.is_empty() {
            for recipe_name in opts.recipes {
                if recipe_name.contains("==") {
                    let mut elems = recipe_name.split("==");
                    let recipe = elems.next().unwrap();
                    if let Some(version) = elems.next() {
                        trace!(logger => "loading recipe '{}', version = {}", recipe, version);
                        recipes_to_build.push((
                            self.recipes.load(recipe).context("loading recipe")?,
                            vec![version.to_string()],
                        ));
                    } else {
                        return err!("invalid syntax for recipe - `{}`", recipe_name);
                    }
                } else {
                    trace!(logger => "loading recipe '{}'", recipe_name);
                    let recipe = self.recipes.load(&recipe_name).context("loading recipe")?;
                    let versions_to_build = recipe.metadata.version.versions().to_vec();
                    recipes_to_build.push((recipe, versions_to_build));
                }
            }
        } else {
            warning!(logger => "no recipes to build");
            warning!(logger => "if you meant to build all recipes run `pkger build --all`");
            warning!(logger => "or only specified recipes with `pkger build <RECIPES>...`");
            return Ok(tasks);
        }

        macro_rules! add_task_if_target_found {
            ($target:ident, $recipe:ident, $self:ident, $tasks:ident, $version:ident) => {
                if let Some(target) = $self
                    .config
                    .images
                    .iter()
                    .find(|target| &target.image == $target)
                {
                    $tasks.push(BuildTask::Custom {
                        recipe: $recipe.clone(),
                        target: target.clone(),
                        version: $version.clone(),
                    });
                } else {
                    warning!(logger => "image '{}' not found in configuration", $target);
                }
            };
        }

        if opts.all {
            debug!(logger => "building all recipes for all targets");
            for (recipe, versions_to_build) in &recipes_to_build {
                if recipe.metadata.all_images {
                    for image in &self.config.images {
                        for version in versions_to_build {
                            tasks.push(BuildTask::Custom {
                                target: image.clone(),
                                recipe: recipe.clone(),
                                version: version.clone(),
                            });
                        }
                    }
                } else if !recipe.images().is_empty() {
                    for target_image in recipe.images() {
                        for version in versions_to_build {
                            add_task_if_target_found!(target_image, recipe, self, tasks, version);
                        }
                    }
                } else {
                    warning!(logger => "recipe '{}' has no image targets, skipping", recipe.metadata.name);
                }
            }
        } else if let Some(targets) = &opts.simple {
            debug!(logger => "building only specified recipes for simple targets");
            for target in targets {
                for (recipe, versions_to_build) in &recipes_to_build {
                    let target = BuildTarget::try_from(target.as_str())?;
                    for version in versions_to_build {
                        tasks.push(BuildTask::Simple {
                            recipe: recipe.clone(),
                            target,
                            version: version.to_string(),
                        })
                    }
                }
            }
        } else if let Some(opt_images) = &opts.images {
            debug!(logger => "building only specified recipes for specified images");
            for (recipe, versions_to_build) in &recipes_to_build {
                if recipe.metadata.all_images {
                    for image in opt_images {
                        for version in versions_to_build {
                            add_task_if_target_found!(image, recipe, self, tasks, version);
                        }
                    }
                } else if !recipe.images().is_empty() {
                    for image in opt_images {
                        // first we check if the recipe contains the image
                        if recipe.images().iter().any(|target| target == image) {
                            // then we fetch the target from configuration images
                            for version in versions_to_build {
                                add_task_if_target_found!(image, recipe, self, tasks, version);
                            }
                        } else {
                            warning!(logger => "image '{}' not found in recipe '{}' targets", image, recipe.metadata.name);
                        }
                    }
                } else {
                    warning!(logger => "recipe '{}' has no image targets, skipping", recipe.metadata.name);
                }
            }
        } else {
            trace!(logger => "building only specified recipes for all targets");
            for (recipe, versions_to_build) in &recipes_to_build {
                if recipe.metadata.all_images {
                    for image in &self.config.images {
                        for version in versions_to_build {
                            tasks.push(BuildTask::Custom {
                                target: image.clone(),
                                recipe: recipe.clone(),
                                version: version.clone(),
                            });
                        }
                    }
                } else if !recipe.images().is_empty() {
                    for target_image in recipe.images() {
                        for version in versions_to_build {
                            add_task_if_target_found!(target_image, recipe, self, tasks, version);
                        }
                    }
                } else {
                    warning!(logger => "recipe {} has no image targets, skipping", recipe.metadata.name);
                }
            }
        }

        Ok(tasks)
    }

    pub async fn process_tasks(
        &mut self,
        tasks: Vec<BuildTask>,
        output_config: AppOutputConfig,
        logger: &mut BoxedCollector,
    ) -> Result<()> {
        debug!(logger => "processing tasks");

        let tasks = self.build_task_queue(tasks, logger)?;
        let results = self.run_tasks(tasks, &output_config, logger).await?;

        let mut task_failed = false;

        // process results
        results.iter().for_each(|res| match res {
                JobResult::Failure { id, duration, reason } => {
                    task_failed = true;
                    error!(logger => "job {} failed, duration: {}s, reason: {}", id, duration.as_secs_f32(), reason);
                }
                JobResult::Success { id, duration, output: out } => {
                    info!(logger => "job {} succeeded, duration: {}s, output: {}", id, duration.as_secs_f32(), out);
                }
            });

        // save image state
        if self.images_state.read().await.has_changed() {
            self.save_images_state(logger).await;
        } else {
            trace!(logger => "images state unchanged, not saving");
        }

        self.cleanup(logger).await;

        if task_failed {
            err!("at least one of the tasks failed")
        } else {
            Ok(())
        }
    }

    fn collector_for_task(
        &self,
        id: &str,
        output_config: &AppOutputConfig,
    ) -> Result<BoxedCollector> {
        let mut collector = if let Some(p) = &output_config.log_dir {
            log::Config::file(p.join(format!("{}.log", id)))
        } else if let Some(p) = &self.config.log_dir {
            log::Config::file(p.join(format!("{}.log", id)))
        } else {
            log::Config::stdout()
        }
        .no_color(output_config.no_color)
        .as_collector()
        .context("initializing output collector")?;

        collector.set_level(output_config.level);

        Ok(collector)
    }

    /// Build a final queue of build tasks
    fn build_task_queue(
        &mut self,
        tasks: Vec<BuildTask>,
        logger: &mut BoxedCollector,
    ) -> Result<VecDeque<Context>> {
        debug!(logger => "building task queue");
        let mut taskmap: HashMap<String, VecDeque<Context>> = HashMap::new();

        // first a map of tasks for each image is built
        for task in tasks {
            let (recipe, image, target, version, is_simple) = match task {
                BuildTask::Custom {
                    recipe,
                    target,
                    version,
                } => {
                    let image = Image::new(
                        target.image.clone(),
                        self.user_images_dir.join(&target.image),
                    );
                    (recipe, image, target, version, false)
                }
                BuildTask::Simple {
                    recipe,
                    target,
                    version,
                } => {
                    let image = Image::try_get_or_new_simple(
                        &self.app_dir.path().join("images"),
                        target,
                        self.config
                            .custom_simple_images
                            .as_ref()
                            .and_then(|c| c.name_for_target(target)),
                    )?;
                    let name = image.name.clone();
                    (
                        recipe,
                        image,
                        ImageTarget::new(name, target, None::<&str>),
                        version,
                        true,
                    )
                }
            };

            let image_name = image.name.clone();

            let ctx = Context::new(
                &self.session_id,
                recipe,
                image,
                self.runtime.connect(),
                target,
                self.config.output_dir.as_path(),
                self.config.recipes_dir.as_path(),
                self.images_state.clone(),
                is_simple,
                self.gpg_key.clone(),
                self.config.ssh.clone(),
                self.proxy.clone(),
                version,
            );
            let id = ctx.id().to_string();
            info!(logger => "adding job {}", id);

            if let Some(tasks) = taskmap.get_mut(&image_name) {
                tasks.push_back(ctx);
            } else {
                taskmap.insert(image_name, VecDeque::from([ctx]));
            }
        }

        let mut total = 0;
        let mut taskmap: Vec<_> = taskmap
            .into_iter()
            .map(|(_, v)| {
                total += v.len();
                v
            })
            .collect();
        let mut taskdeque = VecDeque::new();

        // then the tasks are added one by one from each image so that all target images
        // will be built first rather than spawning jobs of the same image and duplicating work
        let mut processed = 0;
        while processed != total {
            for image_tasks in &mut taskmap {
                if let Some(task) = image_tasks.pop_front() {
                    taskdeque.push_back(task);
                    processed += 1;
                }
            }
        }

        trace!(logger => "final order: {:#?}", taskdeque.iter().map(|c| c.id()).collect::<Vec<_>>());

        Ok(taskdeque)
    }

    async fn get_num_cpus(&self) -> u64 {
        let res = match &self.runtime.connect() {
            RuntimeConnector::Docker(docker) => docker.info().await.ok().map(|info| info.n_cpu),
            RuntimeConnector::Podman(podman) => podman
                .info()
                .await
                .ok()
                .and_then(|info| info.host)
                .and_then(|host| host.cpus)
                .map(|cpus| cpus as u64),
        };

        res.unwrap_or(16)
    }

    async fn run_tasks(
        &self,
        mut tasks: VecDeque<Context>,
        output_config: &AppOutputConfig,
        logger: &mut BoxedCollector,
    ) -> Result<Vec<JobResult>> {
        let mut jobs = FuturesUnordered::new();
        let mut results = vec![];
        let max_jobs = self.get_num_cpus().await;
        let mut running_jobs = 0;
        let total_jobs = tasks.len();
        let mut proccessed_jobs = 0;
        debug!(logger => "cpus: {} (max jobs at once), total jobs to process: {}", max_jobs, total_jobs);
        let start = std::time::SystemTime::now();

        while proccessed_jobs <= total_jobs {
            while running_jobs < max_jobs {
                if let Some(task) = tasks.pop_front() {
                    let collector = self.collector_for_task(task.id(), output_config)?;

                    info!(logger => "starting job {}/{}, id: {}", proccessed_jobs+1, total_jobs, task.id());
                    jobs.push((
                        task.id().to_owned(),
                        task::spawn(JobCtx::Build(task).run(collector)),
                        false,
                    ));
                    running_jobs += 1;
                    proccessed_jobs += 1;
                } else {
                    break;
                }
            }
            let mut all_finished = true;
            let mut should_break = false;
            for (id, job, is_finished) in &mut jobs {
                if *is_finished {
                    continue;
                } else {
                    all_finished = false;
                }
                tokio::select! {
                    res = job => {
                        trace!(logger => "job {} finished", id);
                        running_jobs -= 1;
                        *is_finished = true;
                        if let Err(e) = res {
                            error!(logger => "failed to join task handle, reason: {:?}", e);
                            continue;
                        }
                        results.push(res.unwrap());
                    }
                    _ = self.is_running() => {
                        results.push(
                            JobResult::Failure {
                                id: id.clone(),
                                duration: start.elapsed().unwrap_or_default(),
                                reason: "job cancelled by ctrl-c signal".to_string()
                            }
                        );
                        should_break = true;
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {continue}
                }
            }
            if should_break || all_finished {
                break;
            }
        }

        Ok(results)
    }
    async fn cleanup(&self, logger: &mut BoxedCollector) {
        let runtime = self.runtime.connect();
        match runtime {
            RuntimeConnector::Docker(docker) => {
                match runtime::docker::cleanup(
                    &docker,
                    SESSION_LABEL_KEY,
                    self.session_id.to_string(),
                )
                .await
                {
                    Ok(info) => {
                        trace!(logger => "successfuly removed containers");
                        trace!(logger => "{:?}", info);
                    }
                    Err(e) => {
                        error!(logger => "failed to cleanup containers for session {}, reason {:?}", self.session_id, e);
                    }
                }
            }
            RuntimeConnector::Podman(podman) => {
                match runtime::podman::cleanup(
                    &podman,
                    SESSION_LABEL_KEY,
                    self.session_id.to_string(),
                )
                .await
                {
                    Ok(info) => {
                        trace!(logger => "successfuly removed containers");
                        trace!(logger => "{:?}", info);
                    }
                    Err(e) => {
                        error!(logger => "failed to cleanup containers for session {}, reason {:?}", self.session_id, e);
                    }
                }
            }
        }
    }
}
