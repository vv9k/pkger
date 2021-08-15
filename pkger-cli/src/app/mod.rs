mod build;

use crate::config::Configuration;
use crate::gen;
use crate::opts::{Command, EditObject, ListObject, NewObject, Opts};
use crate::table::{IntoCell, IntoTable};
use pkger_core::docker::DockerConnectionPool;
use pkger_core::gpg::GpgKey;
use pkger_core::image::{state::DEFAULT_STATE_FILE, ImagesState};
use pkger_core::recipe;
use pkger_core::{ErrContext, Error, Result};

use async_rwlock::RwLock;
use colored::Color;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tempdir::TempDir;
use tracing::{error, info, info_span, trace, warn};

// ################################################################################

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

fn open_editor<P: AsRef<Path>>(path: P) -> Result<ExitStatus> {
    let editor = env::var("EDITOR").context("expected $EDITOR env variable set")?;
    let mut cmd = process::Command::new(editor)
        .arg(path.as_ref().to_string_lossy().to_string())
        .spawn()
        .context("failed to open an editor")?;
    cmd.wait().context("failed to wait for child process")
}

fn load_gpg_key(config: &Configuration) -> Result<Option<GpgKey>> {
    if let Some(key) = &config.gpg_key {
        let pass = rpassword::read_password_from_tty(Some("Gpg key password:"))
            .context("failed to read password for gpg key")?;
        if let Some(name) = &config.gpg_name {
            Ok(Some(GpgKey::new(key, name, &pass)?))
        } else {
            err!("missing `gpg_name` field from configuration")
        }
    } else {
        Ok(None)
    }
}

// ################################################################################

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
            Command::List { object } => match object {
                ListObject::Images => self.list_images(),
                ListObject::Recipes { verbose } => self.list_recipes(verbose),
                ListObject::Packages { images } => self.list_packages(images),
            },
            Command::CleanCache => self.clean_cache().await,
            Command::Init { .. } => unreachable!(),
            Command::Edit { object } => self.edit(object),
            Command::New { object } => self.create(object),
        }
    }

    fn create(&self, object: NewObject) -> Result<()> {
        match object {
            NewObject::Image { name } => {
                let path = self.config.images_dir.clone().context("can't create an image when images directory is not specified in the configuration.")?.join(&name);
                if path.exists() {
                    return err!("image `{}` already exists", name);
                }
                println!("creating directory for image ~> `{}`", path.display());
                fs::create_dir(&path).context("failed to create a directory for the image")?;
                let path = path.join("Dockerfile");
                println!("creating a Dockerfile ~> `{}`", path.display());
                fs::write(path, "").context("failed to create a Dockerfile")
            }
            NewObject::Recipe(opts) => {
                let path = self.config.recipes_dir.join(&opts.name);

                if path.exists() {
                    return err!("recipe `{}` already exists", &opts.name);
                }

                let recipe = gen::recipe(opts);
                println!("creating directory for recipe ~> `{}`", path.display());
                fs::create_dir(&path).context("failed to create a directory for the recipe")?;
                let path = path.join("recipe.yml");
                println!("saving recipe ~> `{}`", path.display());
                fs::write(
                    path,
                    &serde_yaml::to_string(&recipe).context("failed to serialize recipe")?,
                )
                .context("failed to save recipe file")
            }
        }
    }

    fn edit(&self, object: EditObject) -> Result<()> {
        match object {
            EditObject::Recipe { name } => {
                let base_path = self.config.recipes_dir.join(&name);
                let path = if base_path.join("recipe.yml").exists() {
                    base_path.join("recipe.yml")
                } else {
                    base_path.join("recipe.yaml")
                };
                if !path.exists() {
                    return err!(
                        "recipe `{}` not found or no `recipe.yml`/`recipe.yaml` file",
                        name
                    );
                }
                let status = open_editor(path)?;
                if let Some(code) = status.code() {
                    process::exit(code);
                }
                Ok(())
            }
            EditObject::Image { name } => {
                if let Some(images_dir) = &self.config.images_dir {
                    let path = images_dir.join(&name).join("Dockerfile");
                    if path.exists() {
                        let status = open_editor(path)?;
                        if let Some(code) = status.code() {
                            process::exit(code);
                        }
                        return Ok(());
                    }
                }
                err!("image `{}` not found", name)
            }
            EditObject::Config => {
                let status = open_editor(&self.config.path)?;
                if let Some(code) = status.code() {
                    process::exit(code);
                }
                Ok(())
            }
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

    fn list_recipes(&self, verbose: bool) -> Result<()> {
        if verbose {
            let mut table = vec![];
            for name in self.recipes.list()? {
                match self.recipes.load(&name) {
                    Ok(recipe) => table.push(vec![
                        recipe
                            .metadata
                            .name
                            .cell()
                            .left()
                            .italic()
                            .color(Color::Blue),
                        recipe
                            .metadata
                            .arch
                            .rpm_name()
                            .cell()
                            .left()
                            .color(Color::White),
                        recipe
                            .metadata
                            .version
                            .cell()
                            .left()
                            .color(Color::BrightYellow),
                        recipe.metadata.license.cell().left().color(Color::White),
                        recipe.metadata.description.cell().left(),
                    ]),
                    Err(e) => warn!(recipe = %name, reason = %format!("{:?}", e)),
                }
            }
            let table = table.into_table().with_headers(vec![
                "Name".cell().bold(),
                "Arch".cell().bold(),
                "Version".cell().bold(),
                "License".cell().bold(),
                "Description".cell().bold(),
            ]);

            table.print();
        } else {
            for name in self.recipes.list()? {
                println!("{}", name);
            }
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
        if let Some(dir) = &self.config.images_dir {
            fs::read_dir(&dir)
                .context("failed to read images directory")?
                .for_each(|e| match e {
                    Ok(e) => {
                        println!("{}", e.file_name().to_string_lossy());
                    }
                    Err(e) => {
                        warn!(reason = %format!("{:?}", e), "invalid entry");
                    }
                });

            Ok(())
        } else {
            return err!("images directory not defined in configuration");
        }
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
