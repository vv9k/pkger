use crate::build::container::Context;
use crate::build::package::sign::{import_gpg_key, upload_gpg_key};
use crate::build::package::{Manifest, Package};
use crate::image::ImageState;
use crate::log::{debug, info, trace, BoxedCollector};
use crate::runtime::container::ExecOpts;
use crate::{ErrContext, Result};

use async_trait::async_trait;
use std::path::{Path, PathBuf};

pub struct Deb;

#[async_trait]
impl Package for Deb {
    fn name(ctx: &Context<'_>, extension: bool) -> String {
        format!(
            "{}-{}-{}.{}{}",
            &ctx.build.recipe.metadata.name,
            &ctx.build.build_version,
            ctx.build.recipe.metadata.release(),
            ctx.build.recipe.metadata.arch.deb_name(),
            if extension { ".deb" } else { "" },
        )
    }

    /// Creates a final DEB package and saves it to `output_dir`
    async fn build(
        ctx: &Context<'_>,
        image_state: &ImageState,
        output_dir: &Path,
        logger: &mut BoxedCollector,
    ) -> Result<PathBuf> {
        let package_name = Self::name(ctx, false);

        info!(logger => "building DEB package {}", package_name);

        let debbld_dir = PathBuf::from("/root/debbuild");
        let tmp_dir = debbld_dir.join("tmp");
        let base_dir = debbld_dir.join(&package_name);
        let deb_dir = base_dir.join("DEBIAN");
        let dirs = [deb_dir.as_path(), tmp_dir.as_path()];

        ctx.create_dirs(&dirs[..], logger)
            .await
            .context("failed to create dirs")?;

        let size_out = ctx
            .checked_exec(
                &ExecOpts::default()
                    .cmd("du -s .")
                    .working_dir(&ctx.build.container_out_dir),
                logger,
            )
            .await
            .context("failed to check size of package files")?
            .stdout
            .join("");
        let size = size_out.split_ascii_whitespace().next();

        let control = ctx
            .build
            .recipe
            .as_deb_control(
                &image_state.image,
                size,
                &ctx.build.build_version,
                *ctx.build.target.build_target(),
                logger,
            )
            .render()
            .context("rendering apkbuild failed")?;
        debug!(logger => "{}", control);

        // Upload install scripts
        if let Some(deb) = &ctx.build.recipe.metadata.deb {
            let mut scripts = vec![];
            let postinst_path = PathBuf::from("./postinst");
            if let Some(postinst) = &deb.postinst_script {
                scripts.push((postinst_path.as_path(), postinst.as_bytes()));
            }
            if !scripts.is_empty() {
                let scripts_paths: String = scripts
                    .iter()
                    .map(|s| s.0.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ");

                ctx.container
                    .upload_files(scripts, &deb_dir, logger)
                    .await
                    .context("failed to upload install scripts to container")?;

                ctx.checked_exec(
                    &ExecOpts::default()
                        .cmd(&format!("chmod 0755 {}", scripts_paths))
                        .working_dir(&deb_dir),
                    logger,
                )
                .await
                .context("failed to change ownership of build scripts")?;
            }
        }

        ctx.container
            .upload_files(
                vec![(PathBuf::from("./control").as_path(), control.as_bytes())],
                &deb_dir,
                logger,
            )
            .await
            .context("failed to upload control file to container")?;

        trace!(logger => "copy source files to build dir");
        ctx.checked_exec(
            &ExecOpts::default()
                .cmd(&format!("cp -rv . {}", base_dir.display()))
                .working_dir(&ctx.build.container_out_dir),
            logger,
        )
        .await
        .context("failed to copy source files to build directory")?;

        let dpkg_deb_opts = if image_state.os.version().parse::<u8>().unwrap_or_default() < 10 {
            "--build"
        } else {
            "--build --root-owner-group"
        };

        ctx.checked_exec(
            &ExecOpts::default().cmd(&format!(
                "dpkg-deb {} {}",
                dpkg_deb_opts,
                base_dir.display()
            )),
            logger,
        )
        .await
        .context("failed to build deb package")?;

        let deb_name = [&package_name, ".deb"].join("");
        let package_file = debbld_dir.join(&deb_name);

        sign_package(ctx, &package_file, logger).await?;

        ctx.container
            .download_files(&package_file, output_dir, logger)
            .await
            .map(|_| output_dir.join(deb_name))
            .context("failed to download finished package")
    }
}

pub async fn sign_package(
    ctx: &Context<'_>,
    package: &Path,
    logger: &mut BoxedCollector,
) -> Result<()> {
    info!(logger => "singing package {}", package.display());
    let gpg_key = if let Some(key) = &ctx.build.gpg_key {
        key
    } else {
        return Ok(());
    };

    let key_file = upload_gpg_key(ctx, gpg_key, &ctx.build.container_tmp_dir, logger)
        .await
        .context("failed to upload gpg key to container")?;

    import_gpg_key(ctx, gpg_key, &key_file, logger)
        .await
        .context("failed to import gpg key")?;

    trace!(logger => "get key id");
    let key_id = ctx
        .checked_exec(
            &ExecOpts::default().cmd("gpg --list-keys --with-colons"),
            logger,
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

    trace!(logger => "add signature");
    ctx.checked_exec(
        &ExecOpts::default().cmd(&format!(
            r#"dpkg-sig -k {} -g "--pinentry-mode=loopback --passphrase {}" --sign {} {}"#,
            key_id,
            gpg_key.pass(),
            gpg_key.name().to_lowercase(),
            package.display()
        )),
        logger,
    )
    .await
    .map(|_| ())
}
