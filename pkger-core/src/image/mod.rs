pub mod os;
pub mod state;

pub use os::find_os;
pub use state::{ImageState, ImagesState};

use crate::recipe::BuildTarget;
use crate::{Error, Result};

use std::collections::HashMap;
use std::convert::AsRef;
use std::fs;
use std::path::{Path, PathBuf};

use tracing::{info_span, trace, warn};

#[derive(Debug, Default)]
/// A wrapper type that contains multiple images found on the filesystem
pub struct Images {
    inner: HashMap<String, Image>,
    path: PathBuf,
}

impl Images {
    /// Initializes an instance of Images without loading them from filesystem
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Images {
            path: path.as_ref().to_path_buf(),
            ..Default::default()
        }
    }

    /// Loads the images from the filesystem by reading each entry in the path and ignoring invalid
    /// entries.
    pub fn load(&mut self) -> Result<()> {
        let span = info_span!("load-images", path = %self.path.display());
        let _enter = span.enter();

        if !self.path.is_dir() {
            return Err(Error::msg(format!(
                "images path `{}` is not a directory",
                self.path.display()
            )));
        }

        for entry in fs::read_dir(self.path.as_path())? {
            match entry {
                Ok(entry) => {
                    let filename = entry.file_name().to_string_lossy().to_string();
                    match Image::load(entry.path()) {
                        Ok(image) => {
                            trace!(image = ?image);
                            self.inner.insert(filename, image);
                        }
                        Err(e) => {
                            warn!(image = %filename, reason = %e, "failed to read image from path")
                        }
                    }
                }
                Err(e) => warn!(reason = %e, "invalid entry"),
            }
        }

        Ok(())
    }

    pub fn images(&self) -> &HashMap<String, Image> {
        &self.inner
    }
}

//####################################################################################################

#[derive(Clone, Debug)]
/// A representation of an image on the filesystem
pub struct Image {
    pub name: String,
    pub path: PathBuf,
}

impl Image {
    pub fn new(images_dir: &Path, target: &BuildTarget) -> Result<Image> {
        let (image, name) = match &target {
            BuildTarget::Rpm => ("centos:latest", "pkger-rpm"),
            BuildTarget::Deb => ("debian:latest", "pkger-deb"),
            BuildTarget::Pkg => ("archlinux", "pkger-pkg"),
            BuildTarget::Gzip => ("ubuntu:latest", "pkger-gzip"),
        };

        let image_dir = images_dir.join(name);
        fs::create_dir_all(&image_dir)?;

        let dockerfile = format!("FROM {}", image);
        fs::write(image_dir.join("Dockerfile"), dockerfile.as_bytes())?;

        Image::load(image_dir)
    }

    /// Loads an `FsImage` from the given `path`
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Image> {
        let path = path.as_ref().to_path_buf();
        if !path.join("Dockerfile").exists() {
            return Err(Error::msg(format!(
                "Dockerfile missing from image `{}`",
                path.display()
            )));
        }
        Ok(Image {
            // we can unwrap here because we know the Dockerfile exists
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            path,
        })
    }
}
