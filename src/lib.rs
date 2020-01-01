#[macro_use]
extern crate failure;
extern crate tar;
use chrono::prelude::Local;
use failure::Error;
use log::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::{self, DirBuilder, DirEntry, File};
use std::path::PathBuf;
use wharf::api::Container;
use wharf::opts::{ContainerBuilderOpts, ExecOpts, ImageBuilderOpts, UploadArchiveOpts};
use wharf::result::CmdOut;
use wharf::Docker;

enum Os {
    Debian,
    Redhat,
}
impl Os {
    fn from(s: &str) -> Result<Os, Error> {
        match s {
            "ubuntu" | "debian" => Ok(Os::Debian),
            "centos" | "redhat" => Ok(Os::Redhat),
            os => Err(format_err!("unknown os {}", os)),
        }
    }
    fn package_manager(self) -> String {
        match self {
            Os::Debian => "apt".to_string(),
            Os::Redhat => "yum".to_string(),
        }
    }
}

#[derive(Deserialize, Debug)]
struct Info {
    name: String,
    version: String,
    revision: String,
    source: String,
    images: Vec<Vec<String>>,
    vendor: Option<String>,
    description: Option<String>,
    depends: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}
#[derive(Deserialize, Debug)]
struct Build {
    steps: Vec<String>,
}
#[derive(Deserialize, Debug)]
struct Install {
    steps: Vec<String>,
    destdir: String,
}
#[derive(Deserialize, Debug)]
struct Recipe {
    info: Info,
    build: Build,
    install: Install,
}
impl Recipe {
    fn new(entry: DirEntry) -> Result<Recipe, Error> {
        let mut path = entry.path();
        path.push("recipe.toml");
        Ok(toml::from_str::<Recipe>(&fs::read_to_string(&path)?)?)
    }
}
type Recipes = HashMap<String, Recipe>;

#[derive(Deserialize, Debug)]
pub struct Config {
    images_dir: String,
    recipes_dir: String,
    output_dir: String,
}

#[derive(Debug)]
struct Image {
    name: String,
    path: PathBuf,
    has_dockerfile: bool,
}
impl Image {
    fn new(entry: DirEntry) -> Image {
        let path = entry.path();
        let has_dockerfile = Image::has_dockerfile(path.clone());
        Image {
            name: entry.file_name().into_string().unwrap_or_default(),
            path,
            has_dockerfile,
        }
    }
    fn has_dockerfile(mut p: PathBuf) -> bool {
        p.push("Dockerfile");
        p.as_path().exists()
    }
}
type Images = HashMap<String, Image>;

#[derive(Debug)]
pub struct Pkger {
    docker: Docker,
    pub config: Config,
    images: Images,
    recipes: Recipes,
}
impl Pkger {
    pub fn new(docker_addr: &str, conf_file: &str) -> Result<Self, Error> {
        let config = toml::from_str::<Config>(&fs::read_to_string(conf_file)?)?;
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
        for _entry in fs::read_dir(p)? {
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
        trace!("{:?}", images);
        Ok(images)
    }

    fn parse_recipes_dir(p: &str) -> Result<Recipes, Error> {
        trace!("parsing recipes dir - {}", p);
        let mut recipes = HashMap::new();
        for _entry in fs::read_dir(p)? {
            if let Ok(entry) = _entry {
                if let Ok(ftype) = entry.file_type() {
                    if ftype.is_dir() {
                        let path = entry.path();
                        match Recipe::new(entry) {
                            Ok(recipe) => {
                                trace!("{:?}", recipe);
                                recipes.insert(recipe.info.name.clone(), recipe);
                            }
                            Err(e) => eprintln!(
                                "directory {} doesn't have a recipe.toml or the recipe is wrong - {}",
                                path.as_path().display(),
                                e
                            ),
                        }
                    }
                }
            }
        }
        trace!("{:?}", recipes);
        Ok(recipes)
    }

    async fn build_image(&self, image: &Image) -> Result<(), Error> {
        trace!("building image {:#?}", image);
        let mut opts = ImageBuilderOpts::new();
        opts.name(&image.name);

        let mut archive_path = PathBuf::from(&self.config.images_dir);
        archive_path.push(format!("{}.tar", &image.name));
        trace!("creating archive in {}", archive_path.as_path().display());
        let file = File::create(archive_path.as_path())
            .map_err(|e| {
                Err::<File, Error>(format_err!(
                    "failed to create temporary archive for image {} in {} - {}",
                    &image.name,
                    archive_path.as_path().display(),
                    e
                ))
            })
            .unwrap();
        let mut archive = tar::Builder::new(file);
        archive.append_dir_all(".", image.path.as_path()).unwrap();
        archive.finish().unwrap();

        let archive_content = fs::read(archive_path.as_path())
            .map_err(|e| {
                Err::<Vec<u8>, Error>(format_err!(
                    "failed to read archived image {} from {} - {}",
                    &image.name,
                    archive_path.as_path().display(),
                    e
                ))
            })
            .unwrap();
        let images = self.docker.images();
        Ok(images
            .build(&archive_content, &opts)
            .await
            .map_err(|e| {
                Err::<(), Error>(format_err!("failed to build image {} - {}", &image.name, e))
            })
            .unwrap())
    }

    async fn image_exists(&self, image: &str) -> bool {
        trace!("checking if image {} exists", image);
        let images = self.docker.images();
        match images.inspect(image).await {
            Ok(_) => true,
            Err(_) => false,
        }
    }
    async fn create_container(&self, image: &Image) -> Result<Container<'_>, Error> {
        trace!("creating container from image {}", &image.name);
        let mut opts = ContainerBuilderOpts::new();
        if !self.image_exists(&image.name).await {
            self.build_image(&image).await?;
        }
        opts.image(&image.name)
            .shell(&["/bin/bash"])
            .cmd(&["/bin/bash"])
            .tty(true)
            .attach_stdout(true)
            .open_stdin(true)
            .attach_stderr(true)
            .attach_stdin(true);
        let name = format!("pkger-{}-{}", &image.name, Local::now().timestamp());
        match self.docker.containers().create(&name, &opts).await {
            Ok(_) => Ok(self.docker.container(&name)),
            Err(e) => Err(format_err!(
                "failed to create container {} with image {} - {}",
                name,
                &image.name,
                e
            )),
        }
    }
    async fn exec_step(
        &self,
        cmd: &[&str],
        container: &'_ Container<'_>,
        build_dir: &str,
    ) -> Result<CmdOut, Error> {
        println!("executing {:?} in {}", cmd, &container.id);
        let mut opts = ExecOpts::new();
        opts.cmd(&cmd)
            .tty(true)
            .working_dir(&build_dir)
            .attach_stderr(true)
            .attach_stdout(true);

        match container.exec(&opts).await {
            Ok(out) if out.info.exit_code != 0 => Err(format_err!(
                "failed to exec step {:?} in container {} - {:?}",
                cmd,
                &container.id,
                out
            )),
            Ok(out) => Ok(out),
            Err(e) => Err(format_err!(
                "failed to exec step {:?} in container {} - {:?}",
                cmd,
                &container.id,
                e
            )),
        }
    }

    pub async fn build_recipe<S: AsRef<str>>(&self, recipe: S) -> Result<(), Error> {
        trace!("building recipe {}", recipe.as_ref());
        match self.recipes.get(recipe.as_ref()) {
            Some(r) => {
                for _image in r.info.images.iter() {
                    let image_name = &_image[0];
                    let image_os = &_image[1];
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
                    trace!("using image - {}, os - {}", image_name, image_os);
                    match self.create_container(&image).await {
                        Ok(container) => {
                            container.start().await?;
                            let build_dir =
                                self.extract_src_in_container(&container, &r.info).await?;
                            self.install_deps(&container, &r.info, image_os).await?;
                            self.execute_build_steps(&container, &r.build, &r.install, &build_dir)
                                .await?;
                            self.download_archive(&container, &r.info, &r.install, image_os)
                                .await?;
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            None => error!(
                "no recipe named {} found in recipes directory {}",
                recipe.as_ref(),
                self.config.recipes_dir
            ),
        }

        Ok(())
    }

    async fn install_deps(
        &self,
        container: &'_ Container<'_>,
        info: &Info,
        os: &str,
    ) -> Result<(), Error> {
        if let Some(dependencies) = &info.depends {
            trace!("installing dependencies - {:?}", dependencies);
            let package_manager = Os::from(os)?.package_manager();
            trace!("using {} as package manager", package_manager);
            match self
                .exec_step(&[&package_manager, "-y", "update"], &container, "/".into())
                .await
            {
                Ok(out) => println!("{}", out.out),
                Err(e) => {
                    return Err(format_err!(
                        "failed to update container {} - {}",
                        &container.id,
                        e
                    ))
                }
            }

            let install_cmd = [
                &vec![package_manager.as_ref(), "-y", "install"][..],
                &dependencies
                    .iter()
                    .map(|s| s.as_ref())
                    .collect::<Vec<&str>>()[..],
            ]
            .concat();
            match self.exec_step(&install_cmd, &container, "/".into()).await {
                Ok(out) => println!("{}", out.out),
                Err(e) => {
                    return Err(format_err!(
                        "failed to install dependencies in container {} - {}",
                        &container.id,
                        e
                    ))
                }
            }
        }
        Ok(())
    }

    // Returns a path to build directory
    async fn extract_src_in_container(
        &self,
        container: &'_ Container<'_>,
        info: &Info,
    ) -> Result<String, Error> {
        let build_dir = format!("/tmp/{}-{}/", info.name, Local::now().timestamp());
        self.exec_step(&["mkdir", &build_dir], &container, "/".into())
            .await?;

        let mut opts = UploadArchiveOpts::new();
        opts.path(&build_dir);

        let src_path = format!(
            "{}/{}/{}",
            &self.config.recipes_dir, &info.name, &info.source
        );
        match fs::read(&src_path) {
            Ok(archive) => {
                container.upload_archive(&archive, &opts).await?;
                Ok(build_dir)
            }
            Err(e) => Err(format_err!("no archive in {} - {}", src_path, e)),
        }
    }

    async fn execute_build_steps(
        &self,
        container: &'_ Container<'_>,
        build: &Build,
        install: &Install,
        build_dir: &str,
    ) -> Result<(), Error> {
        for step in build.steps.iter().chain(install.steps.iter()) {
            println!(
                "{}",
                self.exec_step(
                    &step.split_ascii_whitespace().collect::<Vec<&str>>(),
                    container,
                    &build_dir,
                )
                .await?
                .out
            );
        }
        Ok(())
    }

    async fn download_archive(
        &self,
        container: &'_ Container<'_>,
        info: &Info,
        install: &Install,
        os: &str,
    ) -> Result<(), Error> {
        trace!(
            "downloading archive from {} {}",
            &container.id,
            &install.destdir
        );
        let archive = container.archive_path(&install.destdir).await?;
        let mut out_path = PathBuf::from(&self.config.output_dir);
        out_path.push(os);
        if !out_path.as_path().exists() {
            trace!("creating directory {}", out_path.as_path().display());
            let builder = DirBuilder::new();
            builder.create(out_path.as_path())?;
        }
        out_path.push(format!(
            "{}-{}-{}.tar",
            &info.name, &info.version, &info.revision
        ));

        trace!("saving archive to {}", out_path.as_path().display());
        Ok(fs::write(out_path.as_path(), archive)
            .map_err(|e| {
                Err::<(), Error>(format_err!(
                    "failed saving archive in {} - {}",
                    out_path.as_path().display(),
                    e
                ))
            })
            .unwrap())
    }
}
