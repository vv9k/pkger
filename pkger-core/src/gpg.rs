use crate::{Error, Result};

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct GpgKey {
    path: PathBuf,
    name: String,
    pass: String,
}

impl GpgKey {
    /// Returns a `GpgKey` if the key exists on the filesystem, otherwise
    /// returns an error.
    pub fn new(path: &Path, name: &str, pass: &str) -> Result<Self> {
        if !path.exists() {
            return Err(Error::msg(format!(
                "gpg key does not exist in `{}`",
                path.display()
            )));
        }
        Ok(Self {
            path: path.to_path_buf(),
            name: name.to_owned(),
            pass: pass.to_owned(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn pass(&self) -> &str {
        &self.pass
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}
