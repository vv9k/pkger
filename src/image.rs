use crate::job::{Ctx, OneShotCtx};
use crate::map_return;
use crate::os::Os;
use crate::Result;

use moby::{ContainerOptions, Docker};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::AsRef;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, trace, warn};

#[derive(Debug, Default)]
pub struct Images(HashMap<String, Image>);

impl Images {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut images = Images::default();
        let path = path.as_ref();

        if !path.is_dir() {
            return Ok(images);
        }

        for entry in fs::read_dir(path)? {
            match entry {
                Ok(entry) => {
                    let filename = entry.file_name().to_string_lossy().to_string();
                    match Image::new(entry.path()) {
                        Ok(image) => {
                            images.0.insert(filename, image);
                        }
                        Err(e) => error!("failed to read image from path - {}", e),
                    }
                }
                Err(e) => error!("invalid entry - {}", e),
            }
        }

        Ok(images)
    }

    pub fn images(&self) -> &HashMap<String, Image> {
        &self.0
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
        trace!("checking if image should be rebuilt");
        if let Ok(states) = state.read() {
            if let Some(state) = (*states).images.get(&self.name) {
                if let Ok(entries) = fs::read_dir(self.path.as_path()) {
                    for file in entries {
                        if let Err(e) = file {
                            warn!("error while loading file - {} ", e);
                            continue;
                        }
                        let file = file.unwrap();
                        let path = file.path();
                        let metadata = fs::metadata(path.as_path());
                        if let Err(e) = metadata {
                            warn!(
                                "error while reading metadata of `{}` - {}",
                                path.display(),
                                e
                            );
                            continue;
                        }
                        let metadata = metadata.unwrap();
                        let mod_time = metadata.modified();
                        if let Err(e) = &mod_time {
                            warn!(
                                "error while checking modification time of `{}` - {}",
                                path.display(),
                                e
                            );
                        }
                        let mod_time = mod_time.unwrap();
                        if mod_time > state.timestamp {
                            return None;
                        }
                    }
                }
                return Some(state.to_owned());
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
            "pkger-{}-{}",
            image,
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
        );
        let out = OneShotCtx::new(
            docker,
            &ContainerOptions::builder(image.as_ref())
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

        Ok(ImageState {
            id: id.to_string(),
            image: image.to_string(),
            os: Os::from(os_name, version),
            tag: tag.to_string(),
            timestamp: *timestamp,
        })
    }
}

fn extract_key(out: &str, key: &str) -> Option<String> {
    let key = format!("{}=", key);
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
            map_return!(
                fs::File::create(&self.state_file),
                format!(
                    "failed to create state file in {}",
                    self.state_file.display()
                )
            );
        }
        match serde_cbor::to_vec(&self) {
            Ok(d) => map_return!(
                fs::write(&self.state_file, d),
                format!("failed to save state file in {}", self.state_file.display())
            ),
            Err(e) => return Err(format_err!("failed to serialize image state - {}", e)),
        }
        Ok(())
    }
}
