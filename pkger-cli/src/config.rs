use crate::Result;
use pkger_core::ssh::SshConfig;

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Debug)]
pub struct Configuration {
    pub recipes_dir: PathBuf,
    pub output_dir: PathBuf,
    pub images_dir: Option<PathBuf>,
    pub docker: Option<String>,
    pub gpg_key: Option<PathBuf>,
    pub gpg_name: Option<String>,
    pub ssh: Option<SshConfig>,
}

impl Configuration {
    pub fn load<P: AsRef<Path>>(val: P) -> Result<Self> {
        Ok(serde_yaml::from_slice(&fs::read(val.as_ref())?)?)
    }
}
