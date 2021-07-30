use crate::Result;
use pkger_core::recipe::{deserialize_images, ImageTarget};
use pkger_core::ssh::SshConfig;

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Debug)]
pub struct Configuration {
    pub recipes_dir: PathBuf,
    pub output_dir: PathBuf,
    pub images_dir: Option<PathBuf>,
    pub filter: Option<String>,
    pub docker: Option<String>,
    pub gpg_key: Option<PathBuf>,
    pub gpg_name: Option<String>,
    pub ssh: Option<SshConfig>,
    #[serde(deserialize_with = "deserialize_images")]
    pub images: Vec<ImageTarget>,
}

impl Configuration {
    pub fn load<P: AsRef<Path>>(val: P) -> Result<Self> {
        Ok(serde_yaml::from_slice(&fs::read(val.as_ref())?)?)
    }
}
