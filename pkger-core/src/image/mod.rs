pub mod os;
pub mod state;

use anyhow::Context;
pub use os::find;
pub use state::{ImageState, ImagesState};

use crate::recipe::{BuildTarget, BuildTargetInfo, Os};
use crate::{err, Error, Result};

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

    pub fn simple(target: BuildTarget) -> BuildTargetInfo {
        match target {
            BuildTarget::Rpm => (
                "rockylinux/rockylinux:latest",
                "pkger-rpm",
                Os::new("Rocky", None::<&str>),
            ),
            BuildTarget::Deb => (
                "debian:latest",
                "pkger-deb",
                Os::new("Debian", None::<&str>),
            ),
            BuildTarget::Pkg => ("archlinux", "pkger-pkg", Os::new("Arch", None::<&str>)),
            BuildTarget::Gzip => (
                "debian:latest",
                "pkger-gzip",
                Os::new("Debian", None::<&str>),
            ),
            BuildTarget::Apk => (
                "alpine:latest",
                "pkger-apk",
                Os::new("Alpine", None::<&str>),
            ),
        }
        .into()
    }

    pub fn create_simple(
        images_dir: &Path,
        target: BuildTarget,
        custom_image: Option<&str>,
    ) -> Result<Image> {
        let BuildTargetInfo { image, name, os: _ } = Self::simple(target);
        let image = custom_image.unwrap_or(image);

        let image_dir = images_dir.join(name);
        fs::create_dir_all(&image_dir)?;

        let dockerfile = format!("FROM {}", image);
        fs::write(image_dir.join("Dockerfile"), dockerfile.as_bytes())?;

        Image::try_from_path(image_dir)
    }

    pub fn try_get_or_new_simple(
        images_dir: &Path,
        target: BuildTarget,
        custom_image: Option<&str>,
    ) -> Result<(Image, Os)> {
        let BuildTargetInfo { image: _, name, os } = Self::simple(target);

        let image_dir = images_dir.join(name);
        if image_dir.exists() {
            return Image::try_from_path(image_dir).map(|i| (i, os));
        }

        Self::create_simple(images_dir, target, custom_image).map(|i| (i, os))
    }

    /// Loads an `FsImage` from the given `path`
    pub fn try_from_path<P: AsRef<Path>>(path: P) -> Result<Image> {
        let path = path.as_ref().to_path_buf();
        if !path.join("Dockerfile").exists() {
            return err!("Dockerfile missing from image `{}`", path.display());
        }
        Ok(Image {
            // we can unwrap here because we know the Dockerfile exists
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            path,
        })
    }

    pub fn load_dockerfile(&self) -> Result<String> {
        fs::read_to_string(self.path.join("Dockerfile"))
            .context("failed to read a Dockerfile of image")
    }
}
