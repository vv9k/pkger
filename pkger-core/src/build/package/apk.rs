use crate::build::container::{checked_exec, create_dirs, Context};
use crate::container::ExecOpts;
use crate::image::ImageState;
use crate::{ErrContext, Result};

use std::path::{Path, PathBuf};
use tracing::{debug, info, info_span, trace, Instrument};

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
) -> Result<PathBuf> {
    let package_name = package_name(ctx, false);

    let span = info_span!("APK", package = %package_name);
    async move {
        info!("building APK package");

        let tmp_dir = PathBuf::from(format!("/tmp/{}", package_name));
        let src_dir = tmp_dir.join("src");
        let bld_dir = tmp_dir.join("bld");

        let source_tar_name = [&package_name, ".tar.gz"].join("");
        let source_tar_path = bld_dir.join(&source_tar_name);

        let dirs = [tmp_dir.as_path(), bld_dir.as_path(), src_dir.as_path()];

        create_dirs(ctx, &dirs[..])
            .await
            .context("failed to create dirs")?;

        trace!("copy source files to temporary location");
        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd(&format!("cp -rv . {}", src_dir.display()))
                .working_dir(&ctx.build.container_out_dir)
                .build(),
        )
        .await
        .context("failed to copy source files to temp directory")?;

        trace!("prepare archived source files");
        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd(&format!("tar -zcvf {} .", source_tar_path.display()))
                .working_dir(src_dir.as_path())
                .build(),
        )
        .await?;

        let sources = vec![source_tar_name];
        static BUILD_USER: &str = "builduser";

        let apkbuild = ctx
            .build
            .recipe
            .as_apkbuild(&image_state.image, &sources, &bld_dir)
            .render();
        debug!(APKBUILD = %apkbuild);

        ctx.container
            .upload_files(
                vec![("APKBUILD".to_string(), apkbuild.as_bytes())],
                &bld_dir,
                ctx.build.quiet,
            )
            .await
            .context("failed to upload APKBUILD to container")?;

        trace!("create build user");
        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd(&format!("adduser -D {}", BUILD_USER))
                .build(),
        )
        .await?;
        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd(&format!("passwd -d {}", BUILD_USER))
                .build(),
        )
        .await?;
        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd(&format!("chown -Rv {0}:{0} .", BUILD_USER))
                .working_dir(bld_dir.as_path())
                .build(),
        )
        .await?;
        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd("chmod 644 APKBUILD")
                .working_dir(bld_dir.as_path())
                .build(),
        )
        .await?;
        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd("abuild-keygen -an")
                .user(BUILD_USER)
                .build(),
        )
        .await?;

        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd("cat APKBUILD && ls -l")
                .working_dir(bld_dir.as_path())
                .user(BUILD_USER)
                .build(),
        )
        .await?;

        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd("abuild checksum")
                .working_dir(bld_dir.as_path())
                .user(BUILD_USER)
                .build(),
        )
        .await
        .context("failed to calculate checksum")?;

        trace!("abuild");
        checked_exec(
            ctx,
            &ExecOpts::default()
                .cmd("abuild")
                .working_dir(bld_dir.as_path())
                .user(BUILD_USER)
                .build(),
        )
        .await
        .context("failed to build APK package")?;

        let apk = format!("{}.apk", package_name);
        let apk_path = PathBuf::from(format!(
            "/home/{}/packages/{}/{}/{}",
            BUILD_USER, package_name, ctx.build.recipe.metadata.arch, apk
        ));

        ctx.container
            .download_files(&apk_path, output_dir)
            .await
            .map(|_| output_dir.join(apk))
            .context("failed to download finished package")
    }
    .instrument(span)
    .await
}
