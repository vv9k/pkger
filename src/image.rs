use crate::os::Os;
use crate::Result;
use crate::{
    job::{Ctx, OneShotCtx},
    recipe::{BuildTarget, RecipeTarget},
};

use moby::{image::ImageDetails, ContainerOptions, Docker};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::convert::AsRef;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, info_span, trace, warn, Instrument};

//####################################################################################################

/// Finds out the operating system and version of the image with id `image_id`
pub async fn find_os(image_id: &str, docker: &Docker) -> Result<Os> {
    let span = info_span!("find-os");
    async move {
        let out = OneShotCtx::new(
            docker,
            &ContainerOptions::builder(&image_id)
                .cmd(vec!["cat", "/etc/issue", "/etc/os-release"])
                .build(),
            true,
            true,
        )
        .run()
        .await
        .map_err(|e| anyhow!("failed to check image os - {}", e))?;

        trace!(stderr = %String::from_utf8_lossy(&out.stderr));

        let out = String::from_utf8_lossy(&out.stdout);

        let os_name = extract_key(&out, "ID");
        let version = extract_key(&out, "VERSION_ID");
        Os::new(os_name.unwrap_or_default(), version)
    }
    .instrument(span)
    .await
}

pub fn create_fsimage(images_dir: &Path, target: &BuildTarget) -> Result<FsImage> {
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

    FsImage::new(image_dir)
}

//####################################################################################################

#[derive(Debug, Default)]
/// A wrapper type that contains multiple images found on the filesystem
pub struct FsImages {
    inner: HashMap<String, FsImage>,
    path: PathBuf,
}

impl FsImages {
    /// Initializes an instance of Images without loading them from filesystem
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        FsImages {
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
            return Err(anyhow!(
                "images path `{}` is not a directory",
                self.path.display()
            ));
        }

        for entry in fs::read_dir(self.path.as_path())? {
            match entry {
                Ok(entry) => {
                    let filename = entry.file_name().to_string_lossy().to_string();
                    match FsImage::new(entry.path()) {
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

    pub fn images(&self) -> &HashMap<String, FsImage> {
        &self.inner
    }
}

//####################################################################################################

#[derive(Clone, Debug)]
/// A representation of an image on the filesystem
pub struct FsImage {
    pub name: String,
    pub path: PathBuf,
}

impl FsImage {
    /// Creates a new FsImage from the given path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<FsImage> {
        let path = path.as_ref().to_path_buf();
        if !path.join("Dockerfile").exists() {
            return Err(anyhow!(
                "Dockerfile missing from image `{}`",
                path.display()
            ));
        }
        Ok(FsImage {
            // we can unwrap here because we know the Dockerfile exists
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            path,
        })
    }
}

//####################################################################################################

#[derive(Deserialize, Clone, Debug, Serialize)]
/// Saved state of an image that contains all the metadata of the image
pub struct ImageState {
    pub id: String,
    pub image: String,
    pub tag: String,
    pub os: Os,
    pub timestamp: SystemTime,
    pub details: ImageDetails,
    pub deps: HashSet<String>,
    pub simple: bool,
}

impl ImageState {
    pub async fn new(
        id: &str,
        target: &RecipeTarget,
        tag: &str,
        timestamp: &SystemTime,
        docker: &Docker,
        deps: &HashSet<&str>,
        simple: bool,
    ) -> Result<ImageState> {
        let name = format!(
            "{}-{}",
            target.image(),
            timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );
        let span = info_span!("create-image-state", image = %name);
        async move {
            let os = if let Some(os) = target.image_os() {
                os.clone()
            } else {
                find_os(id, docker).await?
            };
            debug!(os = ?os, "parsed image info");

            let image_handle = docker.images().get(id);
            let details = image_handle.inspect().await?;

            Ok(ImageState {
                id: id.to_string(),
                image: target.image().to_string(),
                os,
                tag: tag.to_string(),
                timestamp: *timestamp,
                details,
                deps: deps.iter().map(|s| s.to_string()).collect(),
                simple,
            })
        }
        .instrument(span)
        .await
    }

    /// Verifies if a given image exists in docker, on connection error returns false
    pub async fn exists(&self, docker: &Docker) -> bool {
        let span = info_span!("check-image-exists", image = %self.image, id = %self.id);
        async move {
            info!("checking if image exists in Docker");
            docker.images().get(&self.id).inspect().await.is_ok()
        }
        .instrument(span)
        .await
    }
}

//####################################################################################################

#[derive(Deserialize, Debug, Serialize)]
pub struct ImagesState {
    /// Contains historical build data of images. Each key-value pair contains an image name and
    /// [ImageState](ImageState) struct representing the state of the image.
    pub images: HashMap<RecipeTarget, ImageState>,
    /// Path to a file containing image state
    pub state_file: PathBuf,
}

impl Default for ImagesState {
    fn default() -> Self {
        ImagesState {
            images: HashMap::new(),
            state_file: PathBuf::from(crate::DEFAULT_STATE_FILE),
        }
    }
}

impl ImagesState {
    /// Tries to initialize images state from the given path
    pub fn try_from_path<P: AsRef<Path>>(state_file: P) -> Result<Self> {
        if !state_file.as_ref().exists() {
            File::create(state_file.as_ref())?;

            return Ok(ImagesState {
                images: HashMap::new(),
                state_file: state_file.as_ref().to_path_buf(),
            });
        }
        let contents = fs::read(state_file.as_ref())?;
        Ok(serde_cbor::from_slice(&contents)?)
    }

    /// Updates the target image with a new state
    pub fn update(&mut self, target: &RecipeTarget, state: &ImageState) {
        self.images.insert(target.clone(), state.clone());
    }

    /// Saves the images state to the filesystem
    pub fn save(&self) -> Result<()> {
        if !Path::new(&self.state_file).exists() {
            trace!(state_file = %self.state_file.display(), "doesn't exist, creating");
            fs::File::create(&self.state_file)
                .map_err(|e| {
                    anyhow!(
                        "failed to create state file in {} - {}",
                        self.state_file.display(),
                        e
                    )
                })
                .map(|_| ())
        } else {
            trace!(state_file = %self.state_file.display(), "file exists, overwriting");
            match serde_cbor::to_vec(&self) {
                Ok(d) => fs::write(&self.state_file, d).map_err(|e| {
                    anyhow!(
                        "failed to save state file in {} - {}",
                        self.state_file.display(),
                        e
                    )
                }),
                Err(e) => return Err(anyhow!("failed to serialize image state - {}", e)),
            }
        }
    }
}

//####################################################################################################

fn extract_key(out: &str, key: &str) -> Option<String> {
    let key = [key, "="].join("");
    if let Some(line) = out.lines().find(|line| line.starts_with(&key)) {
        let line = line.strip_prefix(&key).unwrap();
        if line.starts_with('"') {
            return Some(line.trim_matches('"').to_string());
        }
        return Some(line.to_string());
    }
    None
}
