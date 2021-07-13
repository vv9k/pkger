use crate::archive::{save_tar_gz, tar};
use crate::build::container::Context;
use crate::{ErrContext, Result};

use std::path::{Path, PathBuf};
use tracing::{info, info_span, Instrument};

/// Creates a final GZIP package and saves it to `output_dir` returning the path of the final
/// archive as String.
pub async fn build_gzip(ctx: &Context<'_>, output_dir: &Path) -> Result<PathBuf> {
    let span = info_span!("GZIP");
    let cloned_span = span.clone();
    async move {
        info!("building GZIP package");
        let package = ctx
            .container
            .copy_from(&ctx.build_ctx.container_out_dir)
            .await?;

        let archive = tar::Archive::new(&package[..]);
        let archive_name = format!(
            "{}-{}.tar.gz",
            &ctx.build_ctx.recipe.metadata.name, &ctx.build_ctx.recipe.metadata.version
        );

        cloned_span
            .in_scope(|| {
                save_tar_gz(archive, &archive_name, output_dir)
                    .context("failed to save package as tar.gz")
            })
            .map(|_| output_dir.join(archive_name))
    }
    .instrument(span)
    .await
}
