pub mod os;
pub mod state;

pub use os::find;
pub use state::{ImageState, ImagesState};

use crate::recipe::BuildTarget;
use crate::{Error, Result};

use std::convert::AsRef;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
/// A representation of an image on the filesystem
pub struct Image {
    pub name: String,
    pub path: PathBuf,
}

impl Image {
    pub fn new(name: String, path: PathBuf) -> Self {
        Self { name, path }
    }

    fn simple(target: BuildTarget) -> (&'static str, &'static str) {
        match target {
            BuildTarget::Rpm => ("centos:latest", "pkger-rpm"),
            BuildTarget::Deb => ("ubuntu:latest", "pkger-deb"),
            BuildTarget::Pkg => ("archlinux", "pkger-pkg"),
            BuildTarget::Gzip => ("ubuntu:latest", "pkger-gzip"),
        }
    }

    pub fn create(images_dir: &Path, target: BuildTarget) -> Result<Image> {
        let (image, name) = Self::simple(target);

        let image_dir = images_dir.join(name);
        fs::create_dir_all(&image_dir)?;

        let dockerfile = format!("FROM {}", image);
        fs::write(image_dir.join("Dockerfile"), dockerfile.as_bytes())?;

        Image::try_from_path(image_dir)
    }

    pub fn try_get_or_create(images_dir: &Path, target: BuildTarget) -> Result<Image> {
        let (_, name) = Self::simple(target);

        let image_dir = images_dir.join(name);
        if image_dir.exists() {
            return Image::try_from_path(image_dir);
        }

        Self::create(images_dir, target)
    }

    /// Loads an `FsImage` from the given `path`
    pub fn try_from_path<P: AsRef<Path>>(path: P) -> Result<Image> {
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
