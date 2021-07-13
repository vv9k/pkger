use crate::archive::create_tarball;
use crate::build::container::{checked_exec, create_dirs, Context};
use crate::container::ExecOpts;
use crate::image::ImageState;
use crate::{ErrContext, Result};

use std::path::{Path, PathBuf};
use tracing::{debug, info, info_span, trace, Instrument};

/// Creates a final DEB packages and saves it to `output_dir`
pub async fn build_deb(
    ctx: &Context<'_>,
    image_state: &ImageState,
    output_dir: &Path,
) -> Result<PathBuf> {
    let name = [
        &ctx.build_ctx.recipe.metadata.name,
        "-",
        &ctx.build_ctx.recipe.metadata.version,
    ]
    .join("");
    let arch = ctx.build_ctx.recipe.metadata.arch.deb_name();
    let package_name = [&name, ".", &arch].join("");

    let span = info_span!("DEB", package = %package_name);
    let cloned_span = span.clone();
    async move {
        info!("building DEB package");

        let debbld_dir = PathBuf::from("/root/debbuild");
        let tmp_dir = debbld_dir.join("tmp");
        let base_dir = debbld_dir.join(&package_name);
        let deb_dir = base_dir.join("DEBIAN");
        let dirs = [deb_dir.as_path(), tmp_dir.as_path()];

        create_dirs(&ctx, &dirs[..])
            .await
            .context("failed to create dirs")?;

        let control = ctx
            .build_ctx
            .recipe
            .as_deb_control(&image_state.image)
            .render();
        debug!(control = %control);

        let entries = vec![("./control", control.as_bytes())];
        let control_tar = cloned_span.in_scope(|| create_tarball(entries.into_iter()))?;
        let control_tar_path = tmp_dir.join([&name, "-control.tar"].join(""));

        trace!("copy control archive to container");
        ctx.container
            .inner()
            .copy_file_into(control_tar_path.as_path(), &control_tar)
            .await
            .context("failed to copy archive with control file")?;

        trace!("extract control archive");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!(
                    "tar -xvf {} -C {}",
                    control_tar_path.display(),
                    deb_dir.display(),
                ))
                .build(),
        )
        .await
        .context("failed to extract archive with control file")?;

        trace!("copy source files to build dir");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("cp -rv . {}", base_dir.display()))
                .working_dir(&ctx.build_ctx.container_out_dir)
                .build(),
        )
        .await
        .context("failed to copy source files to build directory")?;

        let dpkg_deb_opts = if image_state.os.version().parse::<u8>().unwrap_or_default() < 10 {
            "--build"
        } else {
            "--build --root-owner-group"
        };

        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!(
                    "dpkg-deb {} {}",
                    dpkg_deb_opts,
                    base_dir.display()
                ))
                .build(),
        )
        .await
        .context("failed to build deb package")?;

        let deb_name = [&package_name, ".deb"].join("");

        ctx.container
            .download_files(debbld_dir.join(&deb_name).as_path(), output_dir)
            .await
            .map(|_| output_dir.join(deb_name))
            .context("failed to download finished package")
    }
    .instrument(span)
    .await
}
