use crate::archive::create_tarball;
use crate::build::container::{checked_exec, create_dirs, Context};
use crate::container::ExecOpts;
use crate::image::ImageState;
use crate::{ErrContext, Result};

use std::{
    fs,
    path::{Path, PathBuf},
};
use tracing::{debug, info, info_span, trace, Instrument};

/// Creates a final RPM package and saves it to `output_dir`
pub(crate) async fn build_rpm(
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
    let release = ctx.build.recipe.metadata.release();
    let arch = ctx.build.recipe.metadata.arch.rpm_name();
    let buildroot_name = [&name, "-", &release, ".", &arch].join("");
    let source_tar = [&name, ".tar.gz"].join("");

    let span = info_span!("RPM", package = %buildroot_name);
    let cloned_span = span.clone();
    async move {
        info!("building RPM package");

        let base_path = PathBuf::from("/root/rpmbuild");
        let specs = base_path.join("SPECS");
        let sources = base_path.join("SOURCES");
        let rpms = base_path.join("RPMS");
        let rpms_arch = rpms.join(&arch);
        let srpms = base_path.join("SRPMS");
        let arch_dir = rpms.join(&arch);
        let rpm_name = format!("{}.rpm", buildroot_name);
        let tmp_buildroot = PathBuf::from(["/tmp/", &buildroot_name].join(""));
        let source_tar_path = sources.join(&source_tar);

        let dirs = [
            specs.as_path(),
            sources.as_path(),
            rpms.as_path(),
            rpms_arch.as_path(),
            srpms.as_path(),
        ];

        create_dirs(&ctx, &dirs[..])
            .await
            .context("failed to create directories")?;

        trace!("copy source files to temporary location");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!(
                    "cp -rv {} {}",
                    ctx.build.container_out_dir.display(),
                    tmp_buildroot.display(),
                ))
                .build(),
        )
        .await
        .context("failed to copy source files to temp directory")?;

        trace!("prepare archived source files");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("tar -zcvf {} .", source_tar_path.display(),))
                .working_dir(tmp_buildroot.as_path())
                .build(),
        )
        .await?;

        trace!("find source file paths");
        let files = checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(r#"find . -type f -o -type l -name "*""#)
                .working_dir(&ctx.build.container_out_dir)
                .build(),
        )
        .await
        .map(|out| {
            out.stdout
                .join("")
                .split_ascii_whitespace()
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_start_matches('.').to_string())
                .collect::<Vec<_>>()
        })
        .context("failed to find source files")?;
        trace!(source_files = ?files);

        let spec = cloned_span.in_scope(|| {
            ctx.build
                .recipe
                .as_rpm_spec(&[source_tar], &files[..], &image_state.image)
                .render()
        });

        let spec_file = [&ctx.build.recipe.metadata.name, ".spec"].join("");
        debug!(spec_file = %spec_file, spec = %spec);

        let entries = vec![(["./", &spec_file].join(""), spec.as_bytes())];
        let spec_tar = cloned_span.in_scope(|| create_tarball(entries.into_iter()))?;

        let spec_tar_path = specs.join([&name, "-spec.tar"].join(""));

        trace!("copy spec archive to container");
        ctx.container
            .inner()
            .copy_file_into(spec_tar_path.as_path(), &spec_tar)
            .await
            .context("failed to copy archive with spec")?;

        trace!("extract spec archive");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!(
                    "tar -xvf {} -C {}",
                    spec_tar_path.display(),
                    specs.display(),
                ))
                .build(),
        )
        .await?;

        trace!("rpmbuild");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!(
                    "setarch {0} rpmbuild -bb --target {0} {1}",
                    ctx.build.recipe.metadata.arch.rpm_name(),
                    specs.join(spec_file).display()
                ))
                .build(),
        )
        .await
        .context("failed to build rpm package")?;

        sign_package(ctx, &arch_dir.join(rpm_name)).await?;

        ctx.container
            .download_files(&arch_dir, output_dir)
            .await
            .map(|_| output_dir.join(format!("{}.rpm", buildroot_name)))
            .context("failed to download finished package")
    }
    .instrument(span)
    .await
}

pub(crate) async fn sign_package(ctx: &Context<'_>, package: &Path) -> Result<()> {
    let span = info_span!("sign", package = %package.display());
    let cloned_span = span.clone();
    async move {
        let gpg_key = if let Some(key) = &ctx.build.gpg_key{
            key
        } else {
            return Ok(());
        };

        const GPG_FILE: &str = "RPM-GPG-SIGN";
        const MACROS_FILE: &str = ".rpmmacros";

        let macros = format!(
            r##"
%_signature gpg
%_gpg_path /root/.gnupg
%_gpg_name {}
%_gpgbin /usr/bin/gpg2
%__gpg_sign_cmd %{{__gpg}} gpg --batch --verbose --pinentry-mode=loopback --passphrase {} -u "%{{_gpg_name}}" -sbo %{{__signature_filename}} --digest-algo sha256 %{{__plaintext_filename}}'
"##,
            gpg_key.name(), gpg_key.pass()
        );
        let key = cloned_span
            .in_scope(|| fs::read(&gpg_key.path()))
            .context("failed reading gpg key")?;

        let entries = vec![
            (format!("./{}", GPG_FILE), key.as_slice()),
            (format!("./{}", MACROS_FILE), macros.as_bytes()),
        ];
        let key_tar = cloned_span
            .in_scope(|| create_tarball(entries.into_iter()))
            .context("failed creating a tarball with gpg key")?;
        let key_path = ctx
            .build
            .container_tmp_dir
            .join(format!("{}.tgz", GPG_FILE));

        trace!("copy signing key to container");
        ctx.container
            .inner()
            .copy_file_into(&key_path, &key_tar)
            .await
            .context("failed to copy archive with signing key")?;

        trace!("extract key archive");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("tar -xf {}", key_path.display(),))
                .working_dir(&ctx.build.container_tmp_dir)
                .build(),
        )
        .await
            .context("failed to extract archive with gpg key")
            ?;

        trace!("import key to gpg");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!(
                    r#"gpg --pinentry-mode=loopback --passphrase {} --import {}"#,
                    gpg_key.pass(), GPG_FILE
                ))
                .working_dir(&ctx.build.container_tmp_dir)
                .build(),
        )
        .await
            .context("failed to import gpg key")
            ?;

        trace!("export public key");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!(
                    r#"gpg --pinentry-mode=loopback --passphrase {} --export -a '{}' > {}.public"#,
                    gpg_key.pass(), gpg_key.name(), GPG_FILE
                ))
                .working_dir(&ctx.build.container_tmp_dir)
                .build(),
        )
            .await
            .context("failed to export public key")
            ?;

        trace!("import key to rpm database");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("rpm --import {}.public", GPG_FILE))
                .working_dir(&ctx.build.container_tmp_dir)
                .build(),
        )
        .await.context("failed importing key to rpm database")?;

        trace!("copy macros");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("cp {} /root/{}", MACROS_FILE, MACROS_FILE))
                .working_dir(&ctx.build.container_tmp_dir)
                .build(),
        )
        .await.context("failed copying macros")?;

        trace!("add signature");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("rpm --addsign {}", package.display()))
                .tty(true)
                .build(),
        )
        .await
        .map(|_| ())
    }
    .instrument(span)
    .await
}
