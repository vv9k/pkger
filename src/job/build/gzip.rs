use crate::job::build::BuildContainerCtx;
use crate::util::save_tar_gz;
use crate::Result;

use std::path::{Path, PathBuf};
use tracing::{info, info_span, Instrument};

impl<'job> BuildContainerCtx<'job> {
    /// Creates a final GZIP package and saves it to `output_dir` returning the path of the final
    /// archive as String.
    pub async fn build_gzip(&self, output_dir: &Path) -> Result<PathBuf> {
        let span = info_span!("GZIP");
        let cloned_span = span.clone();
        async move {
            info!("building GZIP package");
            let package = self.container.copy_from(self.container_out_dir).await?;

            let archive = tar::Archive::new(&package[..]);
            let archive_name = format!(
                "{}-{}.tar.gz",
                &self.recipe.metadata.name, &self.recipe.metadata.version
            );

            cloned_span
                .in_scope(|| {
                    save_tar_gz(archive, &archive_name, output_dir)
                        .map_err(|e| anyhow!("failed to save package as tar.gz - {}", e))
                })
                .map(|_| output_dir.join(archive_name))
        }
        .instrument(span)
        .await
    }
}
