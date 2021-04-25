#[macro_use]
extern crate anyhow;

mod cmd;
mod container;
mod deps;
mod docker;
mod fmt;
mod image;
mod job;
mod opts;
mod os;
mod recipe;
mod util;

use crate::docker::DockerConnectionPool;
use crate::image::{FsImage, FsImages, ImagesState};
use crate::job::{BuildCtx, JobCtx, JobResult};
use crate::opts::{BuildOpts, GenRecipeOpts, PkgerCmd, PkgerOpts};
use crate::recipe::{BuildTarget, Recipes};

pub use anyhow::{Error, Result};
use recipe::{DebRep, MetadataRep, PkgRep, RecipeRep, RpmRep};
use serde::Deserialize;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use tempdir::TempDir;
use tokio::task;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, info_span, trace, warn, Instrument};

static DEFAULT_CONFIG_FILE: &str = ".pkger.toml";
static DEFAULT_STATE_FILE: &str = ".pkger.state";

#[derive(Deserialize, Debug)]
pub struct Config {
    recipes_dir: PathBuf,
    output_dir: PathBuf,
    images_dir: Option<PathBuf>,
    docker: Option<String>,
}
impl Config {
    fn from_path<P: AsRef<Path>>(val: P) -> Result<Self> {
        Ok(toml::from_slice(&fs::read(val.as_ref())?)?)
    }
}

struct Pkger {
    config: Arc<Config>,
    user_images: Arc<Option<FsImages>>,
    recipes: Arc<Recipes>,
    docker: Arc<DockerConnectionPool>,
    images_filter: Arc<Vec<String>>,
    images_state: Arc<RwLock<ImagesState>>,
    is_running: Arc<AtomicBool>,
    simple_targets: Vec<String>,
    _pkger_dir: TempDir,
}

impl Pkger {
    fn new(config: Config) -> Result<Self> {
        let _pkger_dir = create_pkger_dirs()?;
        let user_images = if let Some(path) = &config.images_dir {
            Some(FsImages::new(&path))
        } else {
            None
        };
        let recipes = Recipes::new(&config.recipes_dir);
        let pkger = Pkger {
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
    async fn process_opts(&mut self, opts: PkgerOpts) -> Result<()> {
        match opts.command {
            PkgerCmd::Build(build_opts) => {
                self.load_user_images()?;
                self.load_recipes()?;
                self.process_build_opts(&build_opts)?;
                self.process_tasks().await?;
                self.save_images_state();
                Ok(())
            }
            PkgerCmd::GenRecipe(gen_recipe_opts) => self.gen_recipe(gen_recipe_opts),
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
            .map_err(|e| anyhow!("Failed to initialize docker connection - {}", e))?,
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
                    let image = image::create_fsimage(&location, &target)?;
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
        images: &[(FsImage, BuildTarget)],
        tasks: &mut Vec<JoinHandle<JobResult>>,
    ) {
        for recipe in self.recipes.inner_ref().values() {
            for (image, target) in images {
                debug!(image = %image.name, recipe = %recipe.metadata.name, "spawning task");
                tasks.push(task::spawn(
                    JobCtx::Build(BuildCtx::new(
                        recipe.clone(),
                        (*image).clone(),
                        self.docker.connect(),
                        target.clone(),
                        self.config.clone(),
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
                        JobCtx::Build(BuildCtx::new(
                            recipe.clone(),
                            (*image).clone(),
                            self.docker.connect(),
                            it.target.clone(),
                            self.config.clone(),
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
                return Err(anyhow!("no user images to load"));
            }
        } else {
            return Err(anyhow!("failed to get mutable reference to user images"));
        };
        if let Err(e) = images.load() {
            Err(anyhow!("failed to load images - {}", e))
        } else {
            Ok(())
        }
    }

    fn load_recipes(&mut self) -> Result<()> {
        if let Some(recipes) = Arc::get_mut(&mut self.recipes) {
            if let Err(e) = recipes.load() {
                Err(anyhow!("failed to load recipes - {}", e))
            } else {
                Ok(())
            }
        } else {
            Err(anyhow!(
                "failed to load recipes - couldn't get mutable reference to recipes"
            ))
        }
    }

    fn gen_recipe(&self, opts: Box<GenRecipeOpts>) -> Result<()> {
        let span = info_span!("gen-recipe");
        let _enter = span.enter();
        trace!(opts = ?opts);

        let git = if let Some(url) = opts.git_url {
            let mut git_src = toml::value::Table::new();
            git_src.insert("url".to_string(), toml::Value::String(url));
            if let Some(branch) = opts.git_branch {
                git_src.insert("branch".to_string(), toml::Value::String(branch));
            }
            Some(toml::Value::Table(git_src))
        } else {
            None
        };

        let mut env = toml::value::Map::new();
        if let Some(env_str) = opts.env {
            for kv in env_str.split(',') {
                let mut kv_split = kv.split('=');
                if let Some(k) = kv_split.next() {
                    if let Some(v) = kv_split.next() {
                        if let Some(entry) =
                            env.insert(k.to_string(), toml::Value::String(v.to_string()))
                        {
                            warn!(key = k, old = %entry.to_string(), new = v, "key already exists, overwriting")
                        }
                    } else {
                        warn!(entry = ?kv, "env entry missing a `=`");
                    }
                } else {
                    warn!(entry = kv, "env entry missing a key or `=`");
                }
            }
        }

        macro_rules! vec_as_deps {
            ($it:expr) => {{
                let vec = $it.into_iter().map(toml::Value::from).collect::<Vec<_>>();
                if vec.is_empty() {
                    None
                } else {
                    Some(toml::Value::Array(vec))
                }
            }};
        }

        let deb = DebRep {
            priority: opts.priority,
            installed_size: opts.installed_size,
            built_using: opts.built_using,
            essential: opts.essential,

            pre_depends: vec_as_deps!(opts.pre_depends),
            recommends: vec_as_deps!(opts.recommends),
            suggests: vec_as_deps!(opts.suggests),
            breaks: vec_as_deps!(opts.breaks),
            replaces: vec_as_deps!(opts.replaces.clone()),
            enchances: vec_as_deps!(opts.enchances),
        };

        let rpm = RpmRep {
            obsoletes: vec_as_deps!(opts.obsoletes),
            vendor: opts.vendor,
            icon: opts.icon,
            summary: opts.summary,
            pre_script: None,
            post_script: None,
            preun_script: None,
            postun_script: None,
            config_noreplace: opts.config_noreplace,
        };

        let pkg = PkgRep {
            install: opts.install_script,
            backup: opts.backup_files,
            replaces: vec_as_deps!(opts.replaces),
            optdepends: opts.optdepends,
        };

        let metadata = MetadataRep {
            name: opts.name,
            version: opts.version.unwrap_or_else(|| "1.0.0".to_string()),
            description: opts.description.unwrap_or_else(|| "missing".to_string()),
            license: opts.license.unwrap_or_else(|| "missing".to_string()),
            images: None,

            maintainer: opts.maintainer,
            url: opts.url,
            arch: opts.arch,
            source: opts.source,
            git,
            skip_default_deps: opts.skip_default_deps,
            exclude: opts.exclude,
            group: opts.group,
            release: opts.release,
            epoch: opts.epoch,

            build_depends: vec_as_deps!(opts.build_depends),
            depends: vec_as_deps!(opts.depends),
            conflicts: vec_as_deps!(opts.conflicts),
            provides: vec_as_deps!(opts.provides),

            deb: Some(deb),
            rpm: Some(rpm),
            pkg: Some(pkg),
        };

        let recipe = RecipeRep {
            metadata,
            env: if env.is_empty() { None } else { Some(env) },
            configure: None,
            build: Default::default(),
            install: None,
        };

        let rendered = toml::to_string(&recipe)?;

        if let Some(output_dir) = opts.output_dir {
            fs::write(output_dir.as_path(), rendered)?;
        } else {
            println!("{}", rendered);
        }
        Ok(())
    }
}

fn set_ctrlc_handler(is_running: Arc<AtomicBool>) {
    if let Err(e) = ctrlc::set_handler(move || {
        warn!("got ctrl-c");
        is_running.store(false, Ordering::SeqCst);
    }) {
        error!(reason = %e, "failed to set ctrl-c handler");
    };
}

fn create_pkger_dirs() -> Result<TempDir> {
    let tempdir = TempDir::new("pkger")?;
    let pkger_dir = tempdir.path();
    let images_dir = pkger_dir.join("images");
    if !images_dir.exists() {
        fs::create_dir_all(&images_dir)?;
    }

    Ok(tempdir)
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = PkgerOpts::from_args();

    fmt::setup_tracing(&opts);

    trace!(opts = ?opts);

    // config
    let config_path = opts.config.clone().unwrap_or_else(|| {
        match dirs_next::home_dir() {
            Some(home_dir) => {
                home_dir.join(DEFAULT_CONFIG_FILE).to_string_lossy().to_string()
            }
            None => {
                warn!(path = %DEFAULT_CONFIG_FILE, "current user has no home directory, using default");
                DEFAULT_CONFIG_FILE.to_string()
            }
        }
    });
    trace!(config_path = %config_path);
    let result = Config::from_path(&config_path);
    if let Err(e) = &result {
        error!(reason = %e, config_path = %config_path, "failed to read config file");
        process::exit(1);
    }
    let config = result.unwrap();
    trace!(config = ?config);

    let mut pkger = match Pkger::new(config) {
        Ok(pkger) => pkger,
        Err(e) => {
            error!(reason = %e, "failed to initialize pkger");
            process::exit(1);
        }
    };

    if let Err(e) = pkger.process_opts(opts).await {
        error!(reason = %e, "execution failed");
        process::exit(1);
    }
    Ok(())
}
