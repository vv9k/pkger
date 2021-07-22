use crate::image::find_os;

use crate::docker::{image::ImageDetails, Docker};
use crate::recipe::{Os, RecipeTarget};
use crate::{ErrContext, Result};

use std::collections::{HashMap, HashSet};
use std::convert::AsRef;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, info_span, trace, Instrument};

pub static DEFAULT_STATE_FILE: &str = ".pkger.state";

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
            state_file: PathBuf::from(DEFAULT_STATE_FILE),
        }
    }
}

impl ImagesState {
    /// Tries to initialize images state from the given path, if the path doesn't exist creates
    /// a new ImagesState.
    pub fn try_from_path<P: AsRef<Path>>(state_file: P) -> Result<Self> {
        let state_file = state_file.as_ref();
        if !state_file.exists() {
            debug!("state file doesn't exist");
            return Ok(ImagesState {
                images: HashMap::new(),
                state_file: state_file.to_path_buf(),
            });
        }
        debug!("loading state");
        let contents =
            fs::read(state_file).context("failed to read images state file from the filesystem")?;
        let state =
            serde_cbor::from_slice(&contents).context("failed to deserialize images state")?;

        Ok(state)
    }

    /// Updates the target image with a new state
    pub fn update(&mut self, target: &RecipeTarget, state: &ImageState) {
        self.images.insert(target.clone(), state.clone());
    }

    /// Saves the images state to the filesystem
    pub fn save(&self) -> Result<()> {
        trace!("saving images state");
        serde_cbor::to_vec(&self)
            .context("failed to serialize image state")
            .and_then(|d| fs::write(&self.state_file, d).context("failed to save state file"))
    }
}
