use std::path::{Path, PathBuf};

use crate::build::container::Context;
use crate::image::ImageState;
use crate::log::BoxedCollector;
use crate::recipe::BuildTarget;
use crate::Result;

pub mod apk;
pub mod deb;
pub mod gzip;
pub mod pkg;
pub mod rpm;
mod sign;

pub async fn build(
    ctx: &Context<'_>,
    image_state: &ImageState,
    output_dir: &Path,
    output: &mut BoxedCollector,
) -> Result<PathBuf> {
    match ctx.build.target.build_target() {
        BuildTarget::Gzip => gzip::build(ctx, output_dir, output).await,
        BuildTarget::Rpm => rpm::build(ctx, image_state, output_dir, output).await,
        BuildTarget::Deb => deb::build(ctx, image_state, output_dir, output).await,
        BuildTarget::Pkg => pkg::build(ctx, image_state, output_dir, output).await,
        BuildTarget::Apk => apk::build(ctx, image_state, output_dir, output).await,
    }
}
