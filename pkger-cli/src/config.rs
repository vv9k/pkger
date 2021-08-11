use crate::Result;
use pkger_core::recipe::{deserialize_images, ImageTarget};
use pkger_core::ssh::SshConfig;
use pkger_core::ErrContext;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
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
    #[serde(skip_serializing)]
    #[serde(skip_deserializing)]
    pub path: PathBuf,
}

impl Configuration {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        serde_yaml::from_slice(&fs::read(path).context("failed to read configuration file")?)
            .context("failed to deserialize configuration file")
            .map(|mut cfg: Configuration| {
                cfg.path = path.to_path_buf();
                cfg
            })
    }

    pub fn save(&self) -> Result<()> {
        fs::write(
            &self.path,
            &serde_yaml::to_string(&self).context("failed to serialize configuration file")?,
        )
        .context("failed to save configuration file")
        .map(|_| ())
    }
}
