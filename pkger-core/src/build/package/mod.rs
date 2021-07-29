pub mod deb;
pub mod gzip;
pub mod pkg;
pub mod rpm;
mod sign;

use crate::build::container::Context;
use crate::image::ImageState;
use crate::recipe::BuildTarget;
use crate::Result;

use std::path::{Path, PathBuf};

pub async fn create_package(
    ctx: &Context<'_>,
    image_state: &ImageState,
    output_dir: &Path,
) -> Result<PathBuf> {
    match ctx.build.target.build_target() {
        BuildTarget::Rpm => rpm::build(ctx, image_state, output_dir).await,
        BuildTarget::Gzip => gzip::build(ctx, output_dir).await,
        BuildTarget::Deb => deb::build(ctx, image_state, output_dir).await,
        BuildTarget::Pkg => pkg::build(ctx, image_state, output_dir).await,
    }
}
