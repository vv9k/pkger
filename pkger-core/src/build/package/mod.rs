use crate::build::container::Context;
use crate::image::ImageState;
use crate::log::BoxedCollector;
use crate::recipe::BuildTarget;
use crate::Result;

use pkgspec_core::Manifest;

pub mod apk;
pub mod deb;
pub mod gzip;
pub mod pkg;
pub mod rpm;
mod sign;

use async_trait::async_trait;
use std::path::{Path, PathBuf};

#[async_trait]
pub trait Package {
    fn name(ctx: &Context<'_>, extension: bool) -> String;
    async fn build(
        ctx: &Context<'_>,
        image_state: &ImageState,
        output_dir: &Path,
        logger: &mut BoxedCollector,
    ) -> Result<PathBuf>;
}

pub async fn build(
    ctx: &Context<'_>,
    image_state: &ImageState,
    output_dir: &Path,
    output: &mut BoxedCollector,
) -> Result<PathBuf> {
    match ctx.build.target.build_target() {
        BuildTarget::Gzip => gzip::Gzip::build(ctx, image_state, output_dir, output).await,
        BuildTarget::Rpm => rpm::Rpm::build(ctx, image_state, output_dir, output).await,
        BuildTarget::Deb => deb::Deb::build(ctx, image_state, output_dir, output).await,
        BuildTarget::Pkg => pkg::Pkg::build(ctx, image_state, output_dir, output).await,
        BuildTarget::Apk => apk::Apk::build(ctx, image_state, output_dir, output).await,
    }
}
