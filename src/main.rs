use failure::Error;
use std::path::PathBuf;
use std::fs;
use std::collections::HashMap;
use std::env;
use wharf::{opts::ContainerBuilderOpts, Docker};
use serde::Deserialize;

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
    steps: Vec<String>
}


#[derive(Deserialize, Debug)]
struct Install {
    steps: Vec<String>
}


#[derive(Deserialize, Debug)]
struct Recipe {
    info: Info,
    build: Build,
    install: Install,
}


#[derive(Deserialize, Debug)]
struct Config {
   images_dir: String,
   packages_dir: String,
   output_dir: String,
}

#[derive(Debug)]
struct Image {
    name: String,
    path: PathBuf,
}
type Images = Vec<Image>;

#[derive(Debug)]
struct Pkger<'d> {
    docker: &'d Docker,
    config: Config,
    images: Images,
}
impl<'d> Pkger<'d> {
    pub fn new(docker: &'d Docker) -> Result<Self, Error> {
        let config = toml::from_str::<Config>(&fs::read_to_string("/home/wojtek/dev/rust/pkger/tmp/conf.toml")?)?; 
        let images = Pkger::parse_images_dir(&config.images_dir)?;
        Ok(Pkger { docker, config, images })
    }

    fn parse_images_dir(p: &str) -> Result<Images, Error> {
        unimplemented!()

    }

}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let toml = toml::from_str::<Recipe>(&fs::read_to_string("/home/wojtek/dev/rust/pkger/tmp/recipe.toml")?)?;
    let d = Docker::new("http://0.0.0.0:2376")?;

    let pkgr = Pkger::new(&d)?;
    println!("{:?}", pkgr.config);

    println!("{:?}", toml);

    

    Ok(())
}
