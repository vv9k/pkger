use crate::build::container::{checked_exec, create_dirs, Context};
use crate::build::package::sign::{import_gpg_key, upload_gpg_key};
use crate::container::ExecOpts;
use crate::image::ImageState;
use crate::{ErrContext, Result};

use std::path::{Path, PathBuf};
use tracing::{debug, info, info_span, trace, Instrument};

/// Creates a final DEB packages and saves it to `output_dir`
pub async fn build(
    ctx: &Context<'_>,
    image_state: &ImageState,
    output_dir: &Path,
) -> Result<PathBuf> {
    let name = [
        &ctx.build.recipe.metadata.name,
        "-",
        &ctx.build.recipe.metadata.version,
    ]
    .join("");
    let arch = ctx.build.recipe.metadata.arch.deb_name();
    let package_name = [&name, ".", arch].join("");

    let span = info_span!("DEB", package = %package_name);
    async move {
        info!("building DEB package");

        let debbld_dir = PathBuf::from("/root/debbuild");
        let tmp_dir = debbld_dir.join("tmp");
        let base_dir = debbld_dir.join(&package_name);
        let deb_dir = base_dir.join("DEBIAN");
        let dirs = [deb_dir.as_path(), tmp_dir.as_path()];

        create_dirs(ctx, &dirs[..])
            .await
            .context("failed to create dirs")?;

        let size_out = checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd("du -s .")
                .working_dir(&ctx.build.container_out_dir)
                .build(),
        )
        .await
        .context("failed to check size of package files")?
        .stdout
        .join("");
        let size = size_out.split_ascii_whitespace().next();

        let control = ctx
            .build
            .recipe
            .as_deb_control(&image_state.image, size.as_deref())
            .render();
        debug!(control = %control);

        ctx.container
            .upload_files(
                vec![("./control", control.as_bytes())],
                &deb_dir,
                ctx.build.quiet,
            )
            .await
            .context("failed to upload control file to container")?;

        trace!("copy source files to build dir");
        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd(&format!("cp -rv . {}", base_dir.display()))
                .working_dir(&ctx.build.container_out_dir)
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
            ctx,
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
        let package_file = debbld_dir.join(&deb_name);

        sign_package(ctx, &package_file).await?;

        ctx.container
            .download_files(&package_file, output_dir)
            .await
            .map(|_| output_dir.join(deb_name))
            .context("failed to download finished package")
    }
    .instrument(span)
    .await
}

pub(crate) async fn sign_package(ctx: &Context<'_>, package: &Path) -> Result<()> {
    let span = info_span!("sign", package = %package.display());
    async move {
        let gpg_key = if let Some(key) = &ctx.build.gpg_key {
            key
        } else {
            return Ok(());
        };

        let key_file = upload_gpg_key(ctx, gpg_key, &ctx.build.container_tmp_dir)
            .await
            .context("failed to upload gpg key to container")?;

        import_gpg_key(ctx, gpg_key, &key_file)
            .await
            .context("failed to import gpg key")?;

        trace!("get key id");
        let key_id = checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd("gpg --list-keys --with-colons")
                .build(),
        )
        .await
        .map(|out| {
            let stdout = out.stdout.join("");
            for line in stdout.split('\n') {
                if !line.contains(gpg_key.name()) {
                    continue;
                }

                return line.split(':').nth(7).map(ToString::to_string);
            }
            None
        })
        .context("failed to get gpg key id")?
        .unwrap_or_default();

        trace!("add signature");
        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd(&format!(
                    r#"dpkg-sig -k {} -g "--pinentry-mode=loopback --passphrase {}" --sign {} {}"#,
                    key_id,
                    gpg_key.pass(),
                    gpg_key.name().to_lowercase(),
                    package.display()
                ))
                .build(),
        )
        .await
        .map(|_| ())
    }
    .instrument(span)
    .await
}
