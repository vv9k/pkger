use crate::container::ExecOpts;
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
                .map_err(|e| anyhow!("failed to create directories - {}", e))?;

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
            .map_err(|e| anyhow!("failed to copy source file to temp dir - {}", e))?;

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
                        .cmd(r#"find . -type f -name "*""#)
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
                .map_err(|e| anyhow!("failed to find source files - {}", e))?;
            trace!(source_files = ?files);

            let spec = cloned_span.in_scope(|| {
                self.recipe
                    .as_rpm_spec(&[source_tar], &files[..], &image_state.image)
                    .render()
            });

            let spec_file = [&self.recipe.metadata.name, ".spec"].join("");
            debug!(spec_file = %spec_file, spec = %spec);

            let entries = vec![(["./", &spec_file].join(""), spec.as_bytes())];
            let spec_tar = cloned_span.in_scope(|| create_tar_archive(entries.into_iter()))?;

            let spec_tar_path = specs.join([&name, "-spec.tar"].join(""));

            trace!("copy spec archive to container");
            self.container
                .inner()
                .copy_file_into(spec_tar_path.as_path(), &spec_tar)
                .await
                .map_err(|e| anyhow!("failed to copy archive with spec - {}", e))?;

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

            self.checked_exec(&ExecOpts::default().cmd("ls -l /root/rpmbuild/").build())
                .await?;

            trace!("rpmbuild");
            self.checked_exec(
                &ExecOpts::default()
                    .cmd(&format!("rpmbuild -bb {}", specs.join(spec_file).display()))
                    .build(),
            )
            .await
            .map_err(|e| anyhow!("failed to build rpm package - {}", e))?;

            self.container
                .download_files(rpms.join(&arch).as_path(), output_dir)
                .await
                .map(|_| output_dir.join(format!("{}.rpm", buildroot_name)))
                .map_err(|e| anyhow!("failed to download files - {}", e))
        }
        .instrument(span)
        .await
    }
}
