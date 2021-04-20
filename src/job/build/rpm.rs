use crate::image::ImageState;
use crate::job::build::BuildContainerCtx;
use crate::util::create_tar_archive;
use crate::Result;

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
        let release = if let Some(release) = &self.recipe.metadata.release {
            release
        } else {
            "0"
        };
        let arch = self.recipe.metadata.rpm_arch();
        let buildroot_name = [&name, "-", &release, ".", &arch].join("");
        let source_tar = [&name, ".tar.gz"].join("");

        let span = info_span!("RPM", package = %buildroot_name);

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
        self.checked_exec(&format!(
            "cp -rv {} {}",
            self.container_out_dir.display(),
            tmp_buildroot.display(),
        ))
        .instrument(span.clone())
        .await?;

        trace!(parent: &span, "prepare archived source files");
        self.checked_exec(&format!(
            "cd {} && tar -zcvf {} .",
            tmp_buildroot.display(),
            source_tar_path.display(),
        ))
        .instrument(span.clone())
        .await?;

        trace!(parent: &span, "find source file paths");
        let files = self
            .checked_exec(&format!(
                r#"cd {} && find . -type f -maxdepth 1 -name "*""#,
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
        let dirs = self
            .checked_exec(&format!(
                r#"cd {} && find . -type d -maxdepth 1 -name "*""#, //rpmbuild automatically includes all child files and dirs
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
        trace!(parent: &span, source_files = ?files);

        let spec = span.in_scope(|| {
            self.recipe
                .as_rpm_spec(&[source_tar], &files[..], &dirs[..], &image_state.image)
                .render()
        });

        let spec_file = [&self.recipe.metadata.name, ".spec"].join("");
        debug!(parent: &span, spec_file = %spec_file, spec = %spec);

        let entries = vec![(["./", &spec_file].join(""), spec.as_bytes())];
        let spec_tar = span.in_scope(|| create_tar_archive(entries.into_iter()))?;

        let spec_tar_path = specs.join([&name, "-spec.tar"].join(""));

        trace!(parent: &span, "copy spec archive to container");
        self.container
            .inner()
            .copy_file_into(spec_tar_path.as_path(), &spec_tar)
            .instrument(span.clone())
            .await?;

        trace!(parent: &span, "extract spec archive");
        self.checked_exec(&format!(
            "tar -xvf {} -C {}",
            spec_tar_path.display(),
            specs.display(),
        ))
        .instrument(span.clone())
        .await?;

        trace!(parent: &span, "rpmbuild");
        self.checked_exec(&format!("rpmbuild -bb {}", specs.join(spec_file).display(),))
            .instrument(span.clone())
            .await?;

        self.container
            .download_files(rpms.join(&arch).as_path(), output_dir)
            .instrument(span)
            .await
            .map(|_| output_dir.join(format!("{}.rpm", buildroot_name)))
    }
}
