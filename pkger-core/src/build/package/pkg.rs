use crate::archive::create_tarball;
use crate::build::container::{checked_exec, create_dirs, Context};
use crate::container::ExecOpts;
use crate::image::ImageState;
use crate::{ErrContext, Result};

use std::path::{Path, PathBuf};
use tracing::{debug, info, info_span, trace, Instrument};

/// Creates a final PKG package and saves it to `output_dir`
pub(crate) async fn build_pkg(
    ctx: &Context<'_>,
    image_state: &ImageState,
    output_dir: &Path,
) -> Result<PathBuf> {
    let name = format!(
        "{}-{}",
        &ctx.recipe.metadata.name, &ctx.recipe.metadata.version,
    );
    let arch = ctx.recipe.metadata.arch.pkg_name();
    let package_name = format!("{}-{}-{}", &name, &ctx.recipe.metadata.release(), &arch);

    let span = info_span!("PKG", package = %package_name);
    let cloned_span = span.clone();
    async move {
        info!("building PKG package");

        let tmp_dir = PathBuf::from(format!("/tmp/{}", package_name));
        let src_dir = tmp_dir.join("src");
        let bld_dir = tmp_dir.join("bld");

        let source_tar_name = [&name, ".tar.gz"].join("");
        let source_tar_path = bld_dir.join(source_tar_name);

        let dirs = [tmp_dir.as_path(), bld_dir.as_path(), src_dir.as_path()];

        create_dirs(&ctx, &dirs[..])
            .await
            .context("failed to create dirs")?;

        trace!("copy source files to temporary location");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("cp -rv . {}", src_dir.display()))
                .working_dir(ctx.container_out_dir)
                .build(),
        )
        .await
        .context("failed to copy source files to temp directory")?;

        trace!("prepare archived source files");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("tar -zcvf {} .", source_tar_path.display()))
                .working_dir(src_dir.as_path())
                .build(),
        )
        .await?;

        trace!("calculate source MD5 checksum");
        let sum = checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("md5sum {}", source_tar_path.display()))
                .build(),
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
            .recipe
            .as_pkgbuild(&image_state.image, &sources, &checksums)
            .render();
        debug!(PKGBUILD = %pkgbuild);

        let entries = vec![("PKGBUILD".to_string(), pkgbuild.as_bytes())];
        let pkgbuild_tar = cloned_span.in_scope(|| create_tarball(entries.into_iter()))?;
        let pkgbuild_tar_path = tmp_dir.join("PKGBUILD.tar");

        trace!("copy PKGBUILD archive to container");
        ctx.container
            .inner()
            .copy_file_into(pkgbuild_tar_path.as_path(), &pkgbuild_tar)
            .await
            .context("failed to copy archive with PKGBUILD to container")?;

        trace!("extract PKGBUILD archive");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!(
                    "tar -xvf {} -C {}",
                    pkgbuild_tar_path.display(),
                    bld_dir.display(),
                ))
                .build(),
        )
        .await?;

        trace!("create build user");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("useradd -m {}", BUILD_USER))
                .build(),
        )
        .await?;
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("passwd -d {}", BUILD_USER))
                .build(),
        )
        .await?;
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("chown -Rv {0}:{0} .", BUILD_USER))
                .working_dir(bld_dir.as_path())
                .build(),
        )
        .await?;
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd("chmod 644 PKGBUILD")
                .working_dir(bld_dir.as_path())
                .build(),
        )
        .await?;

        trace!("makepkg");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd("makepkg")
                .working_dir(bld_dir.as_path())
                .user(BUILD_USER)
                .build(),
        )
        .await
        .context("failed to build PKG package")?;

        let pkg = format!("{}.pkg.tar.zst", package_name);
        let pkg_path = bld_dir.join(&pkg);

        ctx.container
            .download_files(&pkg_path, output_dir)
            .await
            .map(|_| output_dir.join(pkg))
            .context("failed to download finished package")
    }
    .instrument(span)
    .await
}
