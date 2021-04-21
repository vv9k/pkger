use crate::image::ImageState;
use crate::job::build::BuildContainerCtx;
use crate::util::create_tar_archive;
use crate::Result;

use std::path::Path;
use std::path::PathBuf;
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
        let arch = self.recipe.metadata.deb_arch();
        let package_name = [&name, ".", &arch].join("");

        let span = info_span!("DEB", package = %package_name);
        let cloned_span = span.clone();
        async move {
            info!("building DEB package");

            let debbld_dir = PathBuf::from("/root/debbuild");
            let tmp_dir = debbld_dir.join("tmp");
            let base_dir = debbld_dir.join(&name);
            let deb_dir = base_dir.join("DEBIAN");
            let dirs = [deb_dir.as_path(), tmp_dir.as_path()];

            self.create_dirs(&dirs[..]).await?;

            let control = self.recipe.as_deb_control(&image_state.image).render();
            debug!(control = %control);

            let entries = vec![("./control", control.as_bytes())];
            let control_tar = cloned_span.in_scope(|| create_tar_archive(entries.into_iter()))?;
            let control_tar_path = tmp_dir.join([&name, "-control.tar"].join(""));

            trace!("copy control archive to container");
            self.container
                .inner()
                .copy_file_into(control_tar_path.as_path(), &control_tar)
                .await?;

            trace!("extract control archive");
            self.checked_exec(
                &format!(
                    "tar -xvf {} -C {}",
                    control_tar_path.display(),
                    deb_dir.display(),
                ),
                None,
            )
            .await?;

            trace!("copy source files to build dir");
            self.checked_exec(
                &format!("cp -rv . {}", base_dir.display()),
                Some(self.container_out_dir),
            )
            .await?;

            let dpkg_deb_opts = if image_state.os.os_ver().parse::<u8>().unwrap_or_default() < 10 {
                "--build"
            } else {
                "--build --root-owner-group"
            };

            self.checked_exec(
                &format!("dpkg-deb {} {}", dpkg_deb_opts, base_dir.display()),
                None,
            )
            .await?;

            let deb_name = [&name, ".deb"].join("");

            self.container
                .download_files(debbld_dir.join(&deb_name).as_path(), output_dir)
                .await
                .map(|_| output_dir.join(deb_name))
        }
        .instrument(span)
        .await
    }
}
