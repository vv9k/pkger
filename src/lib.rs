use failure::Error;
use log::*;
use serde::Deserialize;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use wharf::Docker;

#[derive(Deserialize, Debug)]
struct Info {
    name: String,
    version: String,
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

#[derive(Deserialize, Debug)]
pub struct Config {
    images_dir: String,
    packages_dir: String,
    output_dir: String,
}

#[derive(Debug)]
struct Image {
    name: String,
    path: PathBuf,
    has_dockerfile: bool,
}
impl Image {
    fn new(name: OsString, path: PathBuf) -> Image {
        let has_dockerfile = Image::has_dockerfile(path.clone());
        Image {
            name: name.into_string().unwrap_or_default(),
            path,
            has_dockerfile,
        }
    }
    fn has_dockerfile(mut p: PathBuf) -> bool {
        p.push("Dockerfile");
        p.as_path().exists()
    }
}
type Images = Vec<Image>;

#[derive(Debug)]
pub struct Pkger {
    docker: Docker,
    pub config: Config,
    images: Images,
}
impl Pkger {
    pub fn new(docker_addr: &str, conf_file: &str) -> Result<Self, Error> {
        let config = toml::from_str::<Config>(&fs::read_to_string(conf_file)?)?;
        trace!("{:?}", config);
        let images = Pkger::parse_images_dir(&config.images_dir)?;
        Ok(Pkger {
            docker: Docker::new(docker_addr)?,
            config,
            images,
        })
    }

    fn parse_images_dir(p: &str) -> Result<Images, Error> {
        let mut images = Vec::new();
        for _entry in fs::read_dir(p)? {
            if let Ok(entry) = _entry {
                if let Ok(ftype) = entry.file_type() {
                    if ftype.is_dir() {
                        let image = Image::new(entry.file_name(), entry.path());
                        trace!("{:?}", image);
                        if image.has_dockerfile {
                            images.push(image);
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
        Ok(images)
    }
}
