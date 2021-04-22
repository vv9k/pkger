use crate::job::{Ctx, OneShotCtx};
use crate::os::Os;
use crate::Result;

use moby::{image::ImageDetails, ContainerOptions, Docker};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::AsRef;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, info_span, trace, warn, Instrument};

#[derive(Debug, Default)]
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
                    match Image::new(entry.path()) {
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

#[derive(Clone, Debug)]
pub struct Image {
    pub name: String,
    pub path: PathBuf,
}

impl Image {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Image> {
        let path = path.as_ref().to_path_buf();
        if !path.join("Dockerfile").exists() {
            return Err(anyhow!(
                "Dockerfile missing from image `{}`",
                path.display()
            ));
        }
        Ok(Image {
            // we can unwrap here because we know the Dockerfile exists
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            path,
        })
    }

    /// Checks whether any of the files located at the path of this Image changed since last build.
    /// If shouldn't be rebuilt returns previous `ImageState`.
    pub fn find_cached_state(&self, state: &Arc<RwLock<ImagesState>>) -> Option<ImageState> {
        let span = info_span!("find-image-cache");
        let _enter = span.enter();

        trace!("checking if image should be rebuilt");
        if let Ok(states) = state.read() {
            if let Some(state) = (*states).images.get(&self.name) {
                if let Ok(entries) = fs::read_dir(self.path.as_path()) {
                    for file in entries {
                        if let Err(e) = file {
                            warn!(reason = %e, "error while loading file");
                            continue;
                        }
                        let file = file.unwrap();
                        let path = file.path();
                        let metadata = fs::metadata(path.as_path());
                        if let Err(e) = metadata {
                            warn!(
                                path = %path.display(),
                                reason = %e,
                                "failed to read metadata",
                            );
                            continue;
                        }
                        let metadata = metadata.unwrap();
                        let mod_time = metadata.modified();
                        if let Err(e) = &mod_time {
                            warn!(
                                path = %path.display(),
                                reason = %e,
                                "failed to check modification time",
                            );
                            continue;
                        }
                        let mod_time = mod_time.unwrap();
                        if mod_time > state.timestamp {
                            trace!(path = %path.display(),
                             mod_time = ?mod_time, image_mod_time = ?state.timestamp, "found modified file, not returning cache");
                            return None;
                        }
                    }
                }
                let state = state.to_owned();
                trace!(image_state = ?state, "found cached state");
                return Some(state);
            }
        }
        None
    }
}

#[derive(Deserialize, Clone, Debug, Serialize)]
pub struct ImageState {
    pub id: String,
    pub image: String,
    pub tag: String,
    pub os: Os,
    pub timestamp: SystemTime,
    pub details: ImageDetails,
}

impl ImageState {
    pub async fn new(
        id: &str,
        image: &str,
        tag: &str,
        timestamp: &SystemTime,
        docker: &Docker,
    ) -> Result<ImageState> {
        let name = format!(
            "{}-{}",
            image,
            timestamp
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );
        let span = info_span!("create-image-state", image = %name);
        async move {
            let out = OneShotCtx::new(
                docker,
                &ContainerOptions::builder(image)
                    .name(&name)
                    .cmd(vec!["cat", "/etc/issue", "/etc/os-release"])
                    .build(),
                true,
                false,
            )
            .run()
            .await
            .map_err(|e| anyhow!("failed to check image os - {}", e))?;

            let out = String::from_utf8_lossy(&out.stdout);

            let os_name = extract_key(&out, "ID");
            let version = extract_key(&out, "VERSION_ID");
            let os = Os::from(os_name, version);
            debug!(os = %os.as_ref(), version = %os.os_ver(), "parsed image info");

            let image_handle = docker.images().get(id);
            let details = image_handle.inspect().await?;

            Ok(ImageState {
                id: id.to_string(),
                image: image.to_string(),
                os,
                tag: tag.to_string(),
                timestamp: *timestamp,
                details,
            })
        }
        .instrument(span)
        .await
    }

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

#[derive(Deserialize, Debug, Serialize)]
pub struct ImagesState {
    /// Contains historical build data of images. Each key-value pair contains an image name and
    /// [ImageState](ImageState) struct representing the state of the image.
    pub images: HashMap<String, ImageState>,
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

    pub fn update(&mut self, image: &str, state: &ImageState) {
        self.images.insert(image.to_string(), state.clone());
    }

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
