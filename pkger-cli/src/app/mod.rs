mod build;

use crate::completions;
use crate::config::Configuration;
use crate::gen;
use crate::metadata::{self, PackageMetadata};
use crate::opts::{Command, CopyObject, EditObject, ListObject, NewObject, Opts, RemoveObject, CheckObject};
use crate::table::{Cell, IntoCell, IntoTable};
use pkger_core::gpg::GpgKey;
use pkger_core::image::Image;
use pkger_core::image::{state::DEFAULT_STATE_FILE, ImagesState};
use pkger_core::log::{error, info, trace, warning, BoxedCollector, Level};
use pkger_core::proxy::ProxyConfig;
use pkger_core::recipe;
use pkger_core::runtime::{self, ConnectionPool};
use pkger_core::{ErrContext, Error, Result};

use async_rwlock::RwLock;
use chrono::{offset::TimeZone, SecondsFormat, Utc};
use colored::Color;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time;
use tempdir::TempDir;
use uuid::Uuid;

// ################################################################################

fn set_ctrlc_handler(is_running: Arc<AtomicBool>) {
    if let Err(e) = ctrlc::set_handler(move || {
        warning!("got ctrl-c");
        is_running.store(false, Ordering::SeqCst);
    }) {
        error!("failed to set ctrl-c handler, reason: {}", e);
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

fn system_time_to_date_time(t: time::SystemTime) -> chrono::DateTime<Utc> {
    let (sec, nsec) = match t.duration_since(time::UNIX_EPOCH) {
        Ok(dur) => (dur.as_secs() as i64, dur.subsec_nanos()),
        Err(e) => {
            let dur = e.duration();
            let (sec, nsec) = (dur.as_secs() as i64, dur.subsec_nanos());
            if nsec == 0 {
                (-sec, 0)
            } else {
                (-sec - 1, 1_000_000_000 - nsec)
            }
        }
    };
    Utc.timestamp(sec, nsec)
}

fn init_runtime(
    opts: &Opts,
    config: &Configuration,
    logger: &mut BoxedCollector,
) -> Result<ConnectionPool> {
    // check if docker uri provided as cli arg
    let res = match &opts.runtime_uri {
        Some(uri) => {
            trace!(logger => "using runtime uri from opts, uri: {}", uri);
            if opts.podman || config.podman {
                trace!(logger => "using podman runtime");
                ConnectionPool::podman(uri)
            } else {
                trace!(logger => "using docker runtime");
                ConnectionPool::docker(uri)
            }
        }
        None => {
            // otherwise check if available as config parameter
            if let Some(uri) = &config.runtime_uri {
                trace!(logger => "using docker uri from config, uri {}", uri);
                if opts.podman || config.podman {
                    trace!(logger => "using podman runtime");
                    ConnectionPool::podman(uri)
                } else {
                    trace!(logger => "using docker runtime");
                    ConnectionPool::docker(uri)
                }
            } else {
                trace!(logger => "using default docker uri");
                if opts.podman || config.podman {
                    trace!(logger => "using podman runtime");
                    ConnectionPool::podman(runtime::podman::PODMAN_SOCK)
                } else {
                    trace!(logger => "using docker runtime");
                    if std::path::PathBuf::from(runtime::docker::DOCKER_SOCK).exists() {
                        ConnectionPool::docker(runtime::docker::DOCKER_SOCK)
                    } else {
                        ConnectionPool::docker(runtime::docker::DOCKER_SOCK_SECONDARY)
                    }
                }
            }
        }
    };

    res.context("Failed to initialize docker connection")
}

// ################################################################################

/// A future representing the state of the application. When this future resolves it means
/// the application should not be running any more.
struct IsRunning(Arc<AtomicBool>);
impl std::future::Future for IsRunning {
    type Output = ();
    fn poll(
        self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if !self.0.load(Ordering::Relaxed) {
            std::task::Poll::Ready(())
        } else {
            std::thread::sleep(std::time::Duration::from_millis(50));
            ctx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    }
}

pub struct AppOutputConfig {
    pub level: Level,
    pub log_dir: Option<PathBuf>,
    pub no_color: bool,
}

pub struct Application {
    config: Configuration,
    recipes: recipe::Loader,
    runtime: ConnectionPool,
    images_state: Arc<RwLock<ImagesState>>,
    user_images_dir: PathBuf,
    is_running: Arc<AtomicBool>,
    app_dir: TempDir,
    gpg_key: Option<GpgKey>,
    session_id: Uuid,
    proxy: ProxyConfig,
}

impl Application {
    pub fn new(config: Configuration, opts: &Opts, logger: &mut BoxedCollector) -> Result<Self> {
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
            match ImagesState::load(&state_path).context("failed to load images state") {
                Ok(state) => state,
                Err(e) => {
                    warning!(logger => "{:?}", e);
                    ImagesState::new(&state_path)
                }
            },
        ));

        let runtime = init_runtime(opts, &config, logger)?;

        let app = Application {
            config,
            recipes,
            runtime,
            images_state,
            user_images_dir,
            is_running: Arc::new(AtomicBool::new(true)),
            app_dir,
            gpg_key: None,
            session_id: Uuid::new_v4(),
            proxy: ProxyConfig::from_env(),
        };
        let is_running = app.is_running.clone();
        set_ctrlc_handler(is_running);
        Ok(app)
    }

    pub async fn process_opts(&mut self, opts: Opts, logger: &mut BoxedCollector) -> Result<()> {
        match opts.command {
            Command::Build(build_opts) => {
                if !build_opts.no_sign {
                    self.gpg_key = load_gpg_key(&self.config)?;
                }
                let tasks = self
                    .process_build_opts(build_opts, logger)
                    .context("processing build opts")?;

                let output_config = AppOutputConfig {
                    level: if opts.trace {
                        Level::Trace
                    } else if opts.debug {
                        Level::Debug
                    } else if opts.quiet {
                        Level::Warn
                    } else {
                        Level::Info
                    },
                    log_dir: opts.log_dir,
                    no_color: opts.no_color || self.config.no_color,
                };

                self.process_tasks(tasks, output_config, logger).await?;
                Ok(())
            }
            Command::List {
                object,
                raw,
                verbose,
            } => {
                colored::control::set_override(!raw);
                match object {
                    ListObject::Images => self.list_images(verbose),
                    ListObject::Recipes => self.list_recipes(verbose),
                    ListObject::Packages { images } => self.list_packages(images, verbose),
                }
            }
            Command::CleanCache => self.clean_cache().await,
            Command::Init { .. } => unreachable!(),
            Command::Edit { object } => self.edit(object),
            Command::New { object } => self.create(object, logger),
            Command::Copy { object } => self.copy(object),
            Command::Remove { object, quiet } => self.remove(object, quiet, logger),
            Command::PrintCompletions(opts) => {
                completions::print(&opts);
                Ok(())
            }
            Command::Check { object } => self.check(object, logger).await,
        }
    }

    fn is_running(&self) -> IsRunning {
        IsRunning(self.is_running.clone())
    }

    fn create(&self, object: NewObject, logger: &mut BoxedCollector) -> Result<()> {
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

                let recipe = gen::recipe(opts, logger);
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

    fn copy(&self, object: CopyObject) -> Result<()> {
        fn copy_dir(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
            let dst = dst.as_ref();
            fs::create_dir_all(&dst).context("creating destination directory failed")?;
            for entry in fs::read_dir(src).context("reading source directory failed")? {
                match entry {
                    Ok(entry) => {
                        if let Err(e) = handle_entry(dst, entry) {
                            error!("failed to copy entry entry: {:?}", e);
                        }
                    }
                    Err(e) => {
                        error!("invalid entry: {:?}", e);
                    }
                }
            }
            Ok(())
        }

        fn handle_entry(dst: &Path, entry: fs::DirEntry) -> Result<()> {
            let ty = entry.file_type().context("getting entry type failed")?;
            if ty.is_dir() {
                copy_dir(entry.path(), dst.join(entry.file_name()))
                    .context("copying directory failed")
            } else {
                fs::copy(entry.path(), dst.join(entry.file_name()))
                    .context("copying file failed")
                    .map(|_| ())
            }
        }

        match object {
            CopyObject::Image { source, dest } => {
                if let Some(images_dir) = &self.config.images_dir {
                    let base_path = images_dir.join(&source);
                    let dest_path = images_dir.join(&dest);
                    if !base_path.exists() {
                        return err!("source image `{}` doesn't exists", source);
                    }
                    if dest_path.exists() {
                        return err!("image `{}` already exists", dest);
                    }
                    info!("{} ~> {}", base_path.display(), dest_path.display());
                    copy_dir(base_path, dest_path)
                        .context("failed to copy source image directory")?;
                    info!("done.");
                    Ok(())
                } else {
                    err!("no custom images directory defined in configuration")
                }
            }
            CopyObject::Recipe { source, dest } => {
                let base_path = self.config.recipes_dir.join(&source);
                let dest_path = self.config.recipes_dir.join(&dest);
                if !base_path.exists() {
                    return err!("source recipe `{}` doesn't exists", source);
                }
                if dest_path.exists() {
                    return err!("recipe `{}` already exists", dest);
                }
                info!("{} ~> {}", base_path.display(), dest_path.display());
                copy_dir(base_path, dest_path).context("failed to copy source recipe directory")?;
                info!("done.");
                Ok(())
            }
        }
    }

    async fn clean_cache(&mut self) -> Result<()> {
        info!("clearing cache");
        let mut state = self.images_state.write().await;

        state.clear();
        state.save()?;

        info!("ok");
        Ok(())
    }

    fn list_recipes(&self, verbose: bool) -> Result<()> {
        let mut recipes = self.recipes.list()?;
        recipes.sort_unstable();
        if verbose {
            let mut table = vec![];
            for name in recipes {
                match self.recipes.load(&name) {
                    Ok(recipe) => table.push(vec![
                        recipe
                            .metadata
                            .name
                            .cell()
                            .left()
                            .italic()
                            .color(Color::BrightBlue),
                        recipe
                            .metadata
                            .arch
                            .as_ref()
                            .cell()
                            .left()
                            .color(Color::White),
                        recipe
                            .metadata
                            .version
                            .versions()
                            .join(" ")
                            .cell()
                            .left()
                            .color(Color::BrightYellow),
                        recipe.metadata.license.cell().left().color(Color::White),
                        recipe.metadata.description.cell().left(),
                    ]),
                    Err(e) if verbose => {
                        warning!("failed to load recipe {}, reason: {:?}", name, e)
                    }
                    _ => {}
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
            for name in recipes {
                if self.recipes.load(&name).is_ok() {
                    println!("{}", name);
                }
            }
        }

        Ok(())
    }

    fn list_packages(&self, images_filter: Option<Vec<String>>, verbose: bool) -> Result<()> {
        let mut table = vec![];
        let images = fs::read_dir(&self.config.output_dir)?.filter_map(|e| match e {
            Ok(e) if e.file_type().map(|ty| ty.is_dir()).unwrap_or_default() => Some(e.path()),
            Ok(_) => None, // Skip non directory entries
            Err(e) => {
                if verbose {
                    warning!("invalid entry, reason: {:?}", e);
                }
                None
            }
        });

        let mut images: Vec<_> = if let Some(filter) = images_filter {
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

        images.sort_unstable();

        for image in images {
            let image_name = image
                .file_name()
                .unwrap_or(image.as_os_str())
                .to_string_lossy();

            match fs::read_dir(&image) {
                Ok(packages) => {
                    let mut packages: Vec<_> = packages
                        .filter(|p| match p {
                            Ok(p) => {
                                if let Some(extension) = p.path().extension() {
                                    metadata::SUPPORTED_EXTENSIONS
                                        .contains(&extension.to_string_lossy().as_ref())
                                } else {
                                    false
                                }
                            }
                            Err(e) => {
                                error!(
                                    "failed to list package for image {}, reason {:?}",
                                    image_name, e
                                );
                                false
                            }
                        })
                        .map(|p| p.unwrap())
                        .collect();
                    packages.sort_unstable_by_key(|p| p.file_name());
                    if packages.is_empty() {
                        continue;
                    }

                    table.push(vec![format!("{}:", image_name)
                        .cell()
                        .bold()
                        .color(Color::Blue)
                        .right()]);

                    for package in packages {
                        let path = package.path();
                        match PackageMetadata::try_from_dir_entry(&package)
                            .context("failed to parse package metadata")
                        {
                            Ok(metadata) => {
                                if verbose {
                                    let version = if let Some(release) = metadata.release() {
                                        format!("{}-{}", metadata.version(), release)
                                    } else {
                                        metadata.version().to_string()
                                    };
                                    let timestamp = metadata
                                        .created()
                                        .map(|c| {
                                            system_time_to_date_time(c)
                                                .to_rfc3339_opts(SecondsFormat::Secs, true)
                                        })
                                        .unwrap_or_default();

                                    table.push(vec![
                                        "".cell(),
                                        metadata.name().cell().left().color(Color::BrightBlue),
                                        metadata.package_type().as_ref().cell(),
                                        metadata
                                            .arch()
                                            .as_ref()
                                            .map(|arch| arch.as_ref())
                                            .unwrap_or_default()
                                            .cell()
                                            .color(Color::White),
                                        version.cell().color(Color::BrightYellow),
                                        timestamp.cell().left().color(Color::White),
                                    ]);
                                } else {
                                    table.push(vec![
                                        "".cell(),
                                        path.file_name()
                                            .map(|s| s.to_string_lossy().to_string())
                                            .unwrap_or_default()
                                            .cell()
                                            .left()
                                            .color(Color::BrightBlue),
                                    ]);
                                }
                            }
                            Err(e) if verbose => {
                                warning!("listing package {} {:?}", path.display(), e)
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "failed to list packages for image {}, reason {:?}",
                        image_name, e
                    );
                }
            }
        }

        let headers = if verbose {
            vec![
                "Image".cell().bold(),
                "Name".cell().bold(),
                "Type".cell().bold(),
                "Arch".cell().bold(),
                "Version".cell().bold(),
                "Created".cell().bold(),
            ]
        } else {
            vec!["Image".cell().bold(), "Name".cell().bold()]
        };

        table.into_table().with_header_cells(headers).print();

        Ok(())
    }

    fn list_images(&self, verbose: bool) -> Result<()> {
        fn process_image(image: Image, verbose: bool) -> Result<Vec<Cell>> {
            if verbose {
                let dockerfile = image.load_dockerfile()?;
                if let Some((docker_image, tag)) = dockerfile.lines().next().and_then(|line| {
                    line.to_lowercase().split("from ").nth(1).map(|s| {
                        let mut elems = s.trim().split(':');
                        (
                            elems.next().unwrap().to_string(),
                            elems.next().map(|s| s.to_string()),
                        )
                    })
                }) {
                    return Ok(vec![
                        image.name.cell().left().color(Color::Blue),
                        docker_image.cell().left().color(Color::White),
                        tag.unwrap_or_else(|| "latest".into())
                            .cell()
                            .left()
                            .color(Color::BrightYellow),
                    ]);
                };
            }
            Ok(vec![image.name.cell().left()])
        }

        let mut images = vec![];

        if let Some(dir) = &self.config.images_dir {
            let mut entries: Vec<_> = fs::read_dir(&dir)
                .context("failed to read images directory")?
                .filter(|e| match e {
                    Ok(e) => {
                        if let Ok(ty) = e.file_type() {
                            ty.is_dir()
                        } else {
                            false
                        }
                    }
                    Err(e) if verbose => {
                        warning!("invalid entry, reason: {:?}", e);
                        false
                    }
                    _ => false,
                })
                .map(|e| e.unwrap())
                .collect();

            entries.sort_unstable_by_key(|e| e.file_name());

            entries.into_iter().for_each(|e| {
                match Image::try_from_path(e.path()).and_then(|image| process_image(image, verbose))
                {
                    Ok(out) => {
                        images.push(out);
                    }
                    Err(e) if verbose => {
                        warning!("invalid entry, reason: {:?}", e);
                    }
                    _ => {}
                }
            });

            let headers = if verbose {
                vec![
                    "Name".cell().bold(),
                    "Image".cell().bold(),
                    "Tag".cell().bold(),
                ]
            } else {
                vec!["Name".cell().bold()]
            };

            let table = images.into_table().with_headers(headers);
            table.print();

            Ok(())
        } else {
            return err!("images directory not defined in configuration");
        }
    }

    fn remove(&self, object: RemoveObject, quiet: bool, logger: &mut BoxedCollector) -> Result<()> {
        let (names, base_dir, object) = match object {
            RemoveObject::Images { names } => (names, &self.user_images_dir, "images"),
            RemoveObject::Recipes { names } => (names, &self.config.recipes_dir, "recipes"),
        };
        if names.is_empty() {
            if !quiet {
                warning!(logger => "no {} to remove", object);
            }
            return Ok(());
        }
        for name in names {
            let object_path = base_dir.join(name);
            if let Err(e) = fs::remove_dir_all(&object_path) {
                if !quiet {
                    error!(logger => "failed to remove `{}`: {:?}", object_path.display(), e);
                }
            } else if !quiet {
                info!(logger => "successfully removed `{}`", object_path.display());
            }
        }

        Ok(())
    }

    async fn check(&self, object: CheckObject, logger: &mut BoxedCollector) -> Result<()> {
        match object {
            CheckObject::Connection => {
                match self.runtime.connect() {
                    runtime::RuntimeConnector::Docker(docker) => {
                        match docker.ping().await.context("failed to ping Docker") {
                            Ok(_) => {
                                info!(logger => "connection to runtime OK.");
                                Ok(())
                            },
                            Err(e) => err!("connection failed, reason = {:?}", e)
                        }
                    },
                    runtime::RuntimeConnector::Podman(podman) => {
                        match podman.ping().await.context("failed to ping Podman") {
                            Ok(_) => {
                                info!(logger => "connection to runtime OK.");
                                Ok(())
                            },
                            Err(e) => err!("connection failed, reason = {:?}", e)
                        }
                    }
                }
            }
        }
    }

    async fn save_images_state(&self, logger: &mut BoxedCollector) {
        info!(logger => "saving images state");
        let state = self.images_state.read().await;

        if let Err(e) = state.save() {
            error!(logger => "failed to save image state, reason: {:?}", e);
        }
    }
}
