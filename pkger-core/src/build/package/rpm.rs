use crate::build::container::Context;
use crate::build::package::sign::{import_gpg_key, upload_gpg_key};
use crate::container::ExecOpts;
use crate::image::ImageState;
use crate::log::{debug, info, trace, BoxedCollector};
use crate::recipe::BuildArch;
use crate::{ErrContext, Result};

use std::path::{Path, PathBuf};

pub fn package_name(ctx: &Context<'_>, extension: bool) -> String {
    format!(
        "{}-{}-{}.{}{}",
        &ctx.build.recipe.metadata.name,
        &ctx.build.recipe.metadata.version,
        &ctx.build.recipe.metadata.release(),
        ctx.build.recipe.metadata.arch.rpm_name(),
        if extension { ".rpm" } else { "" },
    )
}

/// Creates a final RPM package and saves it to `output_dir`
pub(crate) async fn build(
    ctx: &Context<'_>,
    image_state: &ImageState,
    output_dir: &Path,
    logger: &mut BoxedCollector,
) -> Result<PathBuf> {
    let recipe = &ctx.build.recipe;
    let arch = recipe.metadata.arch.rpm_name();
    let package_name = package_name(ctx, false);
    let source_tar = [&package_name, ".tar.gz"].join("");

    info!(logger => "building RPM package {}", package_name);

    let base_path = PathBuf::from("/root/rpmbuild");
    let specs = base_path.join("SPECS");
    let sources = base_path.join("SOURCES");
    let rpms = base_path.join("RPMS");
    let rpms_arch = rpms.join(&arch);
    let srpms = base_path.join("SRPMS");
    let arch_dir = rpms.join(&arch);
    let rpm_name = format!("{}.rpm", package_name);
    let tmp_buildroot = PathBuf::from(["/tmp/", &package_name].join(""));
    let source_tar_path = sources.join(&source_tar);

    let dirs = [
        specs.as_path(),
        sources.as_path(),
        rpms.as_path(),
        rpms_arch.as_path(),
        srpms.as_path(),
    ];

    ctx.create_dirs(&dirs[..], logger)
        .await
        .context("failed to create directories")?;

    trace!(logger => "copy source files to temporary location");
    ctx.checked_exec(
        &ExecOpts::default()
            .cmd(&format!(
                "cp -rv {} {}",
                ctx.build.container_out_dir.display(),
                tmp_buildroot.display(),
            ))
            .build(),
        logger,
    )
    .await
    .context("failed to copy source files to temp directory")?;

    trace!(logger => "prepare archived source files");
    ctx.checked_exec(
        &ExecOpts::default()
            .cmd(&format!("tar -zcvf {} .", source_tar_path.display(),))
            .working_dir(tmp_buildroot.as_path())
            .build(),
        logger,
    )
    .await?;

    trace!(logger => "find source file paths");
    let files = ctx
        .checked_exec(
            &ExecOpts::default()
                .cmd(r#"find . -type f -o -type l -name "*""#)
                .working_dir(&ctx.build.container_out_dir)
                .build(),
            logger,
        )
        .await
        .map(|out| {
            out.stdout
                .join("")
                .split('\n')
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_start_matches('.').to_string())
                .collect::<Vec<_>>()
        })
        .context("failed to find source files")?;
    trace!(logger => "source files: {:?}", files);

    let spec = recipe
        .as_rpm_spec(&[source_tar], &files[..], &image_state.image)
        .render();

    let spec_file = [&recipe.metadata.name, ".spec"].join("");
    debug!(logger => "{}", spec);

    ctx.container
        .upload_files(
            vec![(["./", &spec_file].join(""), spec.as_bytes())],
            &specs,
            logger,
        )
        .await
        .context("failed to upload spec file to container")?;

    trace!(logger => "rpmbuild");
    let cmd = if matches!(recipe.metadata.arch, BuildArch::All) {
        format!(
            "rpmbuild -ba --target {0} {1}",
            recipe.metadata.arch.rpm_name(),
            specs.join(spec_file).display()
        )
    } else {
        format!(
            "setarch {0} rpmbuild -ba --target {0} {1}",
            recipe.metadata.arch.rpm_name(),
            specs.join(spec_file).display()
        )
    };
    ctx.checked_exec(&ExecOpts::default().cmd(&cmd).build(), logger)
        .await
        .context("failed to build rpm package")?;

    ctx.checked_exec(
        &ExecOpts::default()
            .cmd(&format!(
                "cp {} {}",
                srpms
                    .join(format!(
                        "{}-{}-{}.src.rpm",
                        &recipe.metadata.name,
                        &recipe.metadata.version,
                        recipe.metadata.release()
                    ))
                    .display(),
                arch_dir.display()
            ))
            .build(),
        logger,
    )
    .await
    .context("failed to copy source rpm to final directory")?;

    sign_package(ctx, &arch_dir.join(rpm_name), logger).await?;

    ctx.container
        .download_files(&arch_dir, output_dir, logger)
        .await
        .map(|_| output_dir.join(format!("{}.rpm", package_name)))
        .context("failed to download finished package")
}

pub(crate) async fn sign_package(
    ctx: &Context<'_>,
    package: &Path,
    logger: &mut BoxedCollector,
) -> Result<()> {
    info!(logger => "signing package {}", package.display());
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

    let macros = format!(
        r##"
%_signature gpg
%_gpg_path /root/.gnupg
%_gpg_name {}
%_gpgbin /usr/bin/gpg2
%__gpg_sign_cmd %{{__gpg}} gpg --batch --verbose --pinentry-mode=loopback --passphrase {} -u "%{{_gpg_name}}" -sbo %{{__signature_filename}} --digest-algo sha256 %{{__plaintext_filename}}'
"##,
        gpg_key.name(),
        gpg_key.pass()
    );

    ctx.container
        .upload_files(vec![("./.rpmmacros", macros.as_bytes())], "/root/", logger)
        .await
        .context("failed to upload rpm macros")?;

    trace!(logger => "export public key");
    ctx.checked_exec(
        &ExecOpts::default()
            .cmd(&format!(
                r#"gpg --pinentry-mode=loopback --passphrase {} --export -a '{}' > public.key"#,
                gpg_key.pass(),
                gpg_key.name()
            ))
            .working_dir(&ctx.build.container_tmp_dir)
            .build(),
        logger,
    )
    .await
    .context("failed to export public key")?;

    trace!(logger => "import key to rpm database");
    ctx.checked_exec(
        &ExecOpts::default()
            .cmd("rpm --import public.key")
            .working_dir(&ctx.build.container_tmp_dir)
            .build(),
        logger,
    )
    .await
    .context("failed importing key to rpm database")?;

    trace!(logger => "add signature");
    ctx.checked_exec(
        &ExecOpts::default()
            .cmd(&format!("rpm --addsign {}", package.display()))
            .build(),
        logger,
    )
    .await
    .map(|_| ())
}
