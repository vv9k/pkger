#[macro_use]
extern crate failure;
use chrono::prelude::Local;
use failure::Error;
use log::*;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::collections::HashMap;
use wharf::opts::{ContainerBuilderOpts, ExecOpts, UploadArchiveOpts};
use wharf::Docker;

#[derive(Deserialize, Debug)]
struct Info {
    name: String,
    version: String,
    source: String,
    images: Vec<String>,
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
}
#[derive(Deserialize, Debug)]
struct Recipe {
    info: Info,
    build: Build,
    install: Install,
}
impl Recipe {
    fn new(entry: fs::DirEntry) -> Result<Recipe, Error> {
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
    fn new(entry: fs::DirEntry) -> Image {
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
                                "directory {} doesn't have a recipe.toml",
                                path.as_path().display()
                            ),
                        }
                    }
                }
            }
        }
        trace!("{:?}", recipes);
        Ok(recipes)
    }

    async fn create_container(&self, image: &str) -> Result<String, Error> {
        let mut opts = ContainerBuilderOpts::new();
        opts.image(image)
            .shell(&["/bin/bash".into()])
            .cmd(&["/bin/bash"]);
        let name = format!("pkger-{}", Local::now().timestamp());
        match self.docker.containers().create(&name, &opts).await {
            Ok(_) => Ok(name),
            Err(e) => Err(format_err!(
                "failed to create container {} with image {} - {}",
                name,
                image,
                e
            )),
        }
    }
    pub async fn exec_step(&self, cmd: &[String], container: &str) -> Result<String, Error> {
        trace!("executing {:?} in {}", cmd, container);
        let mut opts = ExecOpts::new();
        opts.cmd(&cmd).attach_stderr(true).attach_stdout(true).tty(true);
        
        let c = self.docker.container(container);
        c.exec(&opts).await
    }

    pub async fn build_recipe<S: AsRef<str>>(&self, recipe: S) -> Result<(), Error> {
        match self.recipes.get(recipe.as_ref()) {
            Some(r) => {
                for image in r.info.images.iter() {
                    match self.create_container(&image).await {
                        Ok(container) => {
                            self.extract_src_in_container(&container, &r.info).await?;
                            self.execute_build_steps(&container, &r.build).await?;
                        }
                        Err(e) => return Err(format_err!("failed creating container for image {} - {}", image, e)),
                    }
                }
            }
            None => error!("no recipe named {} found in recipes directory {}", recipe.as_ref(), self.config.recipes_dir),
        }

        Ok(())
    }

    // Returns a path to build directory
    async fn extract_src_in_container(&self, container: &str, info: &Info) -> Result<String, Error>  {
        let container = self.docker.container(&container);
        container.start().await?;
        let mut create_dir = ExecOpts::new();
        let build_dir = format!("/tmp/{}-{}", info.name, Local::now().timestamp());
        create_dir.cmd(&["mkdir".into(), build_dir.clone()]).attach_stdout(true).attach_stderr(true);
        container.exec(&create_dir).await?;
        let mut opts = UploadArchiveOpts::new();
        opts.path(&build_dir);

        let archive = fs::read(&info.source)?;
        container.upload_archive(&archive, &opts).await?;

        Ok(build_dir)
    }

    async fn execute_build_steps(&self, container: &str, build: &Build) -> Result<(), Error> {
        for step in build.steps.iter() {
            let out = self.exec_step(&step.split_ascii_whitespace().map(|s| s.to_string()).collect::<Vec<String>>(), container).await?;
            println!("{}", out);
        }

        Ok(())
    }
}
