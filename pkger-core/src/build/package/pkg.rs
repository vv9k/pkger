use crate::build::container::Context;
use crate::build::package::{Manifest, Package};
use crate::image::ImageState;
use crate::log::{debug, info, trace, BoxedCollector};
use crate::runtime::container::ExecOpts;
use crate::{ErrContext, Result};

use async_trait::async_trait;
use std::path::{Path, PathBuf};

pub struct Pkg;

#[async_trait]
impl Package for Pkg {
    fn name(ctx: &Context<'_>, extension: bool) -> String {
        format!(
            "{}-{}-{}-{}{}",
            &ctx.build.recipe.metadata.name,
            &ctx.build.build_version,
            &ctx.build.recipe.metadata.release(),
            ctx.build.recipe.metadata.arch.pkg_name(),
            if extension { ".pkg" } else { "" },
        )
    }

    /// Creates a final PKG package and saves it to `output_dir`
    async fn build(
        ctx: &Context<'_>,
        image_state: &ImageState,
        output_dir: &Path,
        logger: &mut BoxedCollector,
    ) -> Result<PathBuf> {
        let package_name = Self::name(ctx, false);

        info!(logger => "building PKG package {}", package_name);

        let tmp_dir = PathBuf::from(format!("/tmp/{}", package_name));
        let src_dir = tmp_dir.join("src");
        let bld_dir = tmp_dir.join("bld");

        let source_tar_name = [&package_name, ".tar.gz"].join("");
        let source_tar_path = bld_dir.join(source_tar_name);

        let dirs = [tmp_dir.as_path(), bld_dir.as_path(), src_dir.as_path()];

        ctx.create_dirs(&dirs[..], logger)
            .await
            .context("failed to create dirs")?;

        trace!(logger => "copy source files to temporary location");
        ctx.checked_exec(
            &ExecOpts::default()
                .cmd(&format!("cp -rv . {}", src_dir.display()))
                .working_dir(&ctx.build.container_out_dir),
            logger,
        )
        .await
        .context("failed to copy source files to temp directory")?;

        trace!(logger => "prepare archived source files");
        ctx.checked_exec(
            &ExecOpts::default()
                .cmd(&format!("tar -zcvf {} .", source_tar_path.display()))
                .working_dir(src_dir.as_path()),
            logger,
        )
        .await?;

        trace!(logger => "calculate source MD5 checksum");
        let sum = ctx
            .checked_exec(
                &ExecOpts::default().cmd(&format!("md5sum {}", source_tar_path.display())),
                logger,
            )
            .await
            .map(|out| out.stdout.join(""))?;
        let sum = sum
            .split_ascii_whitespace()
            .next()
            .map(|s| s.to_string())
            .context("failed to calculate MD5 checksum of source")?;

        let sources = vec![source_tar_path.to_string_lossy().to_string()];
        let checksums = vec![sum];
        static BUILD_USER: &str = "builduser";

        let pkgbuild = ctx
            .build
            .recipe
            .as_pkgbuild(
                &image_state.image,
                &sources,
                &checksums,
                &ctx.build.build_version,
                *ctx.build.target.build_target(),
                logger,
            )
            .render()
            .context("rendering apkbuild failed")?;
        debug!(logger => "{}", pkgbuild);

        ctx.container
            .upload_files(
                vec![(PathBuf::from("PKGBUILD").as_path(), pkgbuild.as_bytes())],
                &bld_dir,
                logger,
            )
            .await
            .context("failed to upload PKGBUILD to container")?;

        trace!(logger => "create build user");
        ctx.script_exec(
            [
                (
                    ExecOpts::new().cmd(&format!("useradd -m {}", BUILD_USER)),
                    Some("failed to create build user"),
                ),
                (
                    ExecOpts::new().cmd(&format!("passwd -d {}", BUILD_USER)),
                    Some("failed to create build user"),
                ),
                (
                    ExecOpts::new()
                        .cmd(&format!("chown -Rv {0}:{0} .", BUILD_USER))
                        .working_dir(&bld_dir),
                    Some("failed to change ownership of build directory"),
                ),
                (
                    ExecOpts::new()
                        .cmd("chmod 644 PKGBUILD")
                        .working_dir(&bld_dir),
                    Some("failed to change mode of PKGBUILD"),
                ),
                (
                    ExecOpts::new()
                        .cmd("makepkg")
                        .working_dir(&bld_dir)
                        .user(BUILD_USER),
                    Some("failed to makepkg"),
                ),
            ],
            logger,
        )
        .await?;

        let pkg = format!("{}.pkg.tar.zst", package_name);
        let pkg_path = bld_dir.join(&pkg);

        ctx.container
            .download_files(&pkg_path, output_dir, logger)
            .await
            .map(|_| output_dir.join(pkg))
            .context("failed to download finished package")
    }
}
