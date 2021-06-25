use crate::image::ImageState;
use crate::job::build::BuildContainerCtx;
use crate::{Context, Result};
use pkger_core::archive::create_tarball;
use pkger_core::container::ExecOpts;

use std::path::Path;
use std::path::PathBuf;
use tracing::{debug, info, info_span, trace, Instrument};

impl<'job> BuildContainerCtx<'job> {
    /// Creates a final RPM package and saves it to `output_dir`
    pub(crate) async fn build_rpm(
        &self,
        image_state: &ImageState,
        output_dir: &Path,
    ) -> Result<PathBuf> {
        let name = [
            &self.recipe.metadata.name,
            "-",
            &self.recipe.metadata.version,
        ]
        .join("");
        let release = self.recipe.metadata.release();
        let arch = self.recipe.metadata.arch.rpm_name();
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
            let tmp_buildroot = PathBuf::from(["/tmp/", &buildroot_name].join(""));
            let source_tar_path = sources.join(&source_tar);

            let dirs = [
                specs.as_path(),
                sources.as_path(),
                rpms.as_path(),
                rpms_arch.as_path(),
                srpms.as_path(),
            ];

            self.create_dirs(&dirs[..])
                .await
                .context("failed to create directories")?;

            trace!("copy source files to temporary location");
            self.checked_exec(
                &ExecOpts::default()
                    .cmd(&format!(
                        "cp -rv {} {}",
                        self.container_out_dir.display(),
                        tmp_buildroot.display(),
                    ))
                    .build(),
            )
            .await
            .context("failed to copy source files to temp directory")?;

            trace!("prepare archived source files");
            self.checked_exec(
                &ExecOpts::default()
                    .cmd(&format!("tar -zcvf {} .", source_tar_path.display(),))
                    .working_dir(tmp_buildroot.as_path())
                    .build(),
            )
            .await?;

            trace!("find source file paths");
            let files = self
                .checked_exec(
                    &ExecOpts::default()
                        .cmd(r#"find . -type f -o -type l -name "*""#)
                        .working_dir(self.container_out_dir)
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
                self.recipe
                    .as_rpm_spec(&[source_tar], &files[..], &image_state.image)
                    .render()
            });

            let spec_file = [&self.recipe.metadata.name, ".spec"].join("");
            debug!(spec_file = %spec_file, spec = %spec);

            let entries = vec![(["./", &spec_file].join(""), spec.as_bytes())];
            let spec_tar = cloned_span.in_scope(|| create_tarball(entries.into_iter()))?;

            let spec_tar_path = specs.join([&name, "-spec.tar"].join(""));

            trace!("copy spec archive to container");
            self.container
                .inner()
                .copy_file_into(spec_tar_path.as_path(), &spec_tar)
                .await
                .context("failed to copy archive with spec")?;

            trace!("extract spec archive");
            self.checked_exec(
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
            self.checked_exec(
                &ExecOpts::default()
                    .cmd(&format!(
                        "setarch {0} rpmbuild -bb --target {0} {1}",
                        self.recipe.metadata.arch.rpm_name(),
                        specs.join(spec_file).display()
                    ))
                    .build(),
            )
            .await
            .context("failed to build rpm package")?;

            self.container
                .download_files(rpms.join(&arch).as_path(), output_dir)
                .await
                .map(|_| output_dir.join(format!("{}.rpm", buildroot_name)))
                .context("failed to download finished package")
        }
        .instrument(span)
        .await
    }
}
