use crate::image::ImageState;
use crate::job::build::BuildContainerCtx;
use crate::{Context, Result};
use pkger_core::archive::create_tarball;
use pkger_core::container::ExecOpts;

use std::path::{Path, PathBuf};
use tracing::{debug, info, info_span, trace, Instrument};

impl<'job> BuildContainerCtx<'job> {
    /// Creates a final DEB packages and saves it to `output_dir`
    pub(crate) async fn build_deb(
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
        let arch = self.recipe.metadata.arch.deb_name();
        let package_name = [&name, ".", &arch].join("");

        let span = info_span!("DEB", package = %package_name);
        let cloned_span = span.clone();
        async move {
            info!("building DEB package");

            let debbld_dir = PathBuf::from("/root/debbuild");
            let tmp_dir = debbld_dir.join("tmp");
            let base_dir = debbld_dir.join(&package_name);
            let deb_dir = base_dir.join("DEBIAN");
            let dirs = [deb_dir.as_path(), tmp_dir.as_path()];

            self.create_dirs(&dirs[..])
                .await
                .context("failed to create dirs")?;

            let control = self.recipe.as_deb_control(&image_state.image).render();
            debug!(control = %control);

            let entries = vec![("./control", control.as_bytes())];
            let control_tar = cloned_span.in_scope(|| create_tarball(entries.into_iter()))?;
            let control_tar_path = tmp_dir.join([&name, "-control.tar"].join(""));

            trace!("copy control archive to container");
            self.container
                .inner()
                .copy_file_into(control_tar_path.as_path(), &control_tar)
                .await
                .context("failed to copy archive with control file")?;

            trace!("extract control archive");
            self.checked_exec(
                &ExecOpts::default()
                    .cmd(&format!(
                        "tar -xvf {} -C {}",
                        control_tar_path.display(),
                        deb_dir.display(),
                    ))
                    .build(),
            )
            .await
            .context("failed to extract archive with control file")?;

            trace!("copy source files to build dir");
            self.checked_exec(
                &ExecOpts::default()
                    .cmd(&format!("cp -rv . {}", base_dir.display()))
                    .working_dir(self.container_out_dir)
                    .build(),
            )
            .await
            .context("failed to copy source files to build directory")?;

            let dpkg_deb_opts = if image_state.os.version().parse::<u8>().unwrap_or_default() < 10 {
                "--build"
            } else {
                "--build --root-owner-group"
            };

            self.checked_exec(
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

            self.container
                .download_files(debbld_dir.join(&deb_name).as_path(), output_dir)
                .await
                .map(|_| output_dir.join(deb_name))
                .context("failed to download finished package")
        }
        .instrument(span)
        .await
    }
}
