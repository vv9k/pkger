use crate::image::ImageState;
use crate::job::build::BuildContainerCtx;
use crate::util::{create_tar_archive, unpack_archive};
use crate::Result;

use futures::TryStreamExt;
use std::path::Path;
use std::path::PathBuf;
use tracing::{debug, info, info_span, trace, Instrument};

impl<'job> BuildContainerCtx<'job> {
    /// Creates a final RPM package and saves it to `output_dir`
    pub(crate) async fn build_rpm(
        &self,
        image_state: &ImageState,
        output_dir: &Path,
    ) -> Result<()> {
        let name = [
            &self.recipe.metadata.name,
            "-",
            &self.recipe.metadata.version,
        ]
        .join("");
        let revision = if self.recipe.metadata.revision.is_empty() {
            "0"
        } else {
            &self.recipe.metadata.revision
        };
        let arch = if self.recipe.metadata.arch.is_empty() {
            "noarch"
        } else {
            &self.recipe.metadata.arch
        };
        let buildroot_name = [&name, "-", &revision, ".", &arch].join("");
        let source_tar = [&name, ".tar.gz"].join("");

        let span = info_span!("RPM", package = %buildroot_name);
        let _enter = span.enter();

        info!(parent: &span, "building RPM package");

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

        self.create_dirs(&dirs[..]).instrument(span.clone()).await?;

        trace!(parent: &span, "copy source files to temporary location");
        self.container
            .exec(format!(
                "cp -rv {} {}",
                self.container_out_dir.display(),
                tmp_buildroot.display(),
            ))
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "prepare archived source files");
        self.container
            .exec(format!(
                "cd {} && tar -zcvf {} .",
                tmp_buildroot.display(),
                source_tar_path.display(),
            ))
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "find source file paths");
        let files = self
            .container
            .exec(format!(
                r#"cd {} && find . -type f -name "*""#,
                self.container_out_dir.display()
            ))
            .instrument(span.clone())
            .await
            .map(|out| {
                out.stdout
                    .join("")
                    .split_ascii_whitespace()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.trim_start_matches('.').to_string())
                    .collect::<Vec<_>>()
            })?;
        trace!(parent: &span, source_files = %files.join(", "));

        let spec = self
            .recipe
            .as_rpm_spec(&[source_tar], &files[..], &image_state.image)
            .render_owned()?;

        let spec_file = [&self.recipe.metadata.name, ".spec"].join("");
        debug!(parent: &span, spec_file = %spec_file, spec = %spec);

        let entries = vec![(["./", &spec_file].join(""), spec.as_bytes())];
        let spec_tar = async move { create_tar_archive(entries) }
            .instrument(span.clone())
            .await?;

        let spec_tar_path = specs.join([&name, "-spec.tar"].join(""));

        trace!(parent: &span, "copy spec archive to container");
        self.container
            .inner()
            .copy_file_into(spec_tar_path.as_path(), &spec_tar)
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "extract spec archive");
        self.container
            .exec(format!(
                "tar -xvf {} -C {}",
                spec_tar_path.display(),
                specs.display(),
            ))
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "rpmbuild");
        // TODO: check why rpmbuild doesn't extract the source_tar to BUILDROOTt
        self.container
            .exec(format!("rpmbuild -bb {}", specs.join(spec_file).display(),))
            .instrument(span.clone())
            .await?;
        // TODO: verify stderr here to check if build succeded

        let rpm = self
            .container
            .inner()
            .copy_from(rpms.join(&self.recipe.metadata.arch).as_path())
            .try_concat()
            .instrument(span.clone())
            .await?;

        let mut archive = tar::Archive::new(&rpm[..]);

        async move {
            unpack_archive(&mut archive, output_dir)
                .map_err(|e| anyhow!("failed to unpack archive - {}", e))
        }
        .instrument(span.clone())
        .await
    }
}
