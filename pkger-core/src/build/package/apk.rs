use crate::build::container::Context;
use crate::container::{Container, ExecOpts};
use crate::image::ImageState;
use crate::log::{debug, info, trace, BoxedCollector};
use crate::{ErrContext, Result};

use std::path::{Path, PathBuf};

pub fn package_name(ctx: &Context<'_>, extension: bool) -> String {
    format!(
        "{}-{}-r{}{}",
        &ctx.build.recipe.metadata.name,
        &ctx.build.recipe.metadata.version,
        &ctx.build.recipe.metadata.release(),
        if extension { ".apk" } else { "" },
    )
}

/// Creates a final APK package and saves it to `output_dir`
pub(crate) async fn build(
    ctx: &Context<'_>,
    image_state: &ImageState,
    output_dir: &Path,
    logger: &mut BoxedCollector,
) -> Result<PathBuf> {
    let package_name = package_name(ctx, false);

    info!(logger => "building APK package {}", package_name);

    let tmp_dir: PathBuf = ["/tmp", &package_name].into_iter().collect();
    let src_dir = tmp_dir.join("src");
    let bld_dir = tmp_dir.join("bld");

    let source_tar_name = [&package_name, ".tar.gz"].join("");
    let source_tar_path = bld_dir.join(&source_tar_name);

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

    let sources = vec![source_tar_name];
    static BUILD_USER: &str = "builduser";

    let apkbuild = ctx
        .build
        .recipe
        .as_apkbuild(&image_state.image, &sources, &bld_dir, logger)
        .render();
    debug!(logger => "{}", apkbuild);

    ctx.container
        .upload_files(
            vec![("APKBUILD".to_string(), apkbuild.as_bytes())],
            &bld_dir,
            logger,
        )
        .await
        .context("failed to upload APKBUILD to container")?;

    trace!(logger => "create build user");

    let home_dir: PathBuf = ["/home", BUILD_USER].into_iter().collect();
    let abuild_dir = home_dir.join(".abuild");

    ctx.script_exec(
        [
            (
                ExecOpts::new().cmd(&format!("adduser -D {}", BUILD_USER)),
                Some("failed to create a build user"),
            ),
            (
                ExecOpts::new().cmd(&format!("passwd -d {}", BUILD_USER)),
                Some("failed to set password of build user"),
            ),
            (
                ExecOpts::new().cmd(&format!("mkdir {}", abuild_dir.display())),
                None,
            ),
        ],
        logger,
    )
    .await?;

    const SIGNING_KEY: &str = "apk-signing-key";
    let key_path = abuild_dir.join(SIGNING_KEY);
    let uploaded_key = if let Some(key_location) = ctx
        .build
        .recipe
        .metadata
        .apk
        .as_ref()
        .and_then(|apk| apk.private_key.as_deref())
    {
        if let Ok(key) = std::fs::read(&key_location) {
            info!("uploading signing key");
            trace!(logger => "key location: {}", key_location.display());
            ctx.container
                .upload_files([(SIGNING_KEY, key.as_slice())], &abuild_dir, logger)
                .await
                .context("failed to upload signing key")?;
            ctx.checked_exec(
                &ExecOpts::new().cmd(&format!("chmod 600 {}", key_path.display())),
                logger,
            )
            .await
            .context("failed to change mode of signing key")?;
            true
        } else {
            false
        }
    } else {
        false
    };

    ctx.script_exec(
        [
            (
                ExecOpts::new().cmd(&format!(
                    "chown -Rv {0}:{0} {1} {2}",
                    BUILD_USER,
                    bld_dir.display(),
                    abuild_dir.display()
                )),
                Some("failed to change ownership of the build directory"),
            ),
            (
                ExecOpts::new()
                    .cmd("chmod 644 APKBUILD")
                    .working_dir(&bld_dir),
                Some("failed to change mode of APKBUILD"),
            ),
        ],
        logger,
    )
    .await?;

    if !uploaded_key {
        ctx.checked_exec(
            &ExecOpts::new()
                .cmd("abuild-keygen -an")
                .working_dir(&bld_dir)
                .user(BUILD_USER),
            logger,
        )
        .await?;
    } else {
        ctx.checked_exec(
            &ExecOpts::new()
                .cmd(&format!(
                    "echo PACKAGER_PRIVKEY=\"{}\" >> abuild.conf",
                    key_path.display()
                ))
                .working_dir(&abuild_dir)
                .user(BUILD_USER),
            logger,
        )
        .await?;
    }

    ctx.script_exec(
        [
            (
                ExecOpts::new()
                    .cmd("abuild checksum")
                    .working_dir(&bld_dir)
                    .user(BUILD_USER),
                Some("failed to calculate checksum"),
            ),
            (
                ExecOpts::new()
                    .cmd("abuild")
                    .working_dir(&bld_dir)
                    .user(BUILD_USER),
                Some("failed to run abuild"),
            ),
        ],
        logger,
    )
    .await?;

    let apk = format!("{}.apk", package_name);
    let mut apk_path = home_dir.clone();
    apk_path.push("packages");
    apk_path.push(&package_name);
    apk_path.push(ctx.build.recipe.metadata.arch.apk_name());
    apk_path.push(&apk);

    ctx.container
        .download_files(&apk_path, output_dir, logger)
        .await
        .map(|_| output_dir.join(apk))
        .context("failed to download finished package")
}
