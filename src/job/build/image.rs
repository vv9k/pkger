use crate::image::ImageState;
use crate::job::BuildCtx;
use crate::Result;

use futures::StreamExt;
use moby::{image::ImageBuildChunk, BuildOptions};
use std::time::SystemTime;
use tracing::{debug, info, info_span, trace, warn, Instrument};

impl BuildCtx {
    pub async fn image_build(&mut self) -> Result<ImageState> {
        let span = info_span!("image-build");

        async move {
            if let Some(state) = self.image.find_cached_state(&self.image_state) {
                if state.exists(&self.docker).await {
                    trace!("exists");
                    return Ok(state);
                } else {
                    warn!("found cached state but image doesn't exist in docker")
                }
            }

            debug!(image = %self.image.name, "building from scratch");
            let images = self.docker.images();
            let opts = BuildOptions::builder(self.image.path.to_string_lossy().to_string())
                .tag(&format!("{}:latest", &self.image.name))
                .build();

            let mut stream = images.build(&opts);

            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                match chunk {
                    ImageBuildChunk::Error {
                        error,
                        error_detail: _,
                    } => {
                        return Err(anyhow!(error));
                    }
                    ImageBuildChunk::Update { stream } => {
                        info!("{}", stream);
                    }
                    ImageBuildChunk::Digest { aux } => {
                        let state = ImageState::new(
                            &aux.id,
                            &self.image.name,
                            "latest",
                            &SystemTime::now(),
                            &self.docker,
                        )
                        .await?;

                        if let Ok(mut image_state) = self.image_state.write() {
                            (*image_state).update(&self.image.name, &state)
                        }

                        return Ok(state);
                    }
                    _ => {}
                }
            }

            Err(anyhow!("stream ended before image id was received"))
        }
        .instrument(span)
        .await
    }
}
