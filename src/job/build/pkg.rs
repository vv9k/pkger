use crate::image::ImageState;
use crate::job::build::BuildContainerCtx;
use crate::util::create_tar_archive;
use crate::Result;

use std::path::{Path, PathBuf};
use tracing::{debug, info, info_span, trace, Instrument};

impl<'job> BuildContainerCtx<'job> {
    /// Creates a final PKG package and saves it to `output_dir`
    pub(crate) async fn build_pkg(
        &self,
        image_state: &ImageState,
        output_dir: &Path,
    ) -> Result<PathBuf> {
        let name = format!(
            "{}-{}",
            &self.recipe.metadata.name, &self.recipe.metadata.version,
        );
        let arch = self.recipe.metadata.arch.pkg_name();
        let package_name = format!("{}-{}-{}", &name, &self.recipe.metadata.release(), &arch);

        let span = info_span!("PKG", package = %package_name);
        let cloned_span = span.clone();
        async move {
            info!("building PKG package");

            let tmp_dir = PathBuf::from(format!("/tmp/{}", package_name));
            let src_dir = tmp_dir.join("src");
            let bld_dir = tmp_dir.join("bld");

            let source_tar_name = [&name, ".tar.gz"].join("");
            let source_tar_path = bld_dir.join(source_tar_name);

            let dirs = [tmp_dir.as_path(), bld_dir.as_path(), src_dir.as_path()];

            self.create_dirs(&dirs[..])
                .await
                .map_err(|e| anyhow!("failed to create dirs - {}", e))?;

            trace!("copy source files to temporary location");
            self.checked_exec(
                &format!("cp -rv . {}", src_dir.display()),
                Some(self.container_out_dir),
                None,
                None,
            )
            .await
            .map_err(|e| anyhow!("failed to copy source file to temp dir - {}", e))?;

            trace!("prepare archived source files");
            self.checked_exec(
                &format!("tar -zcvf {} .", source_tar_path.display()),
                Some(src_dir.as_path()),
                None,
                None,
            )
            .await?;

            trace!("calculate source MD5 checksum");
            let sum = self
                .checked_exec(
                    &format!("md5sum {}", source_tar_path.display()),
                    None,
                    None,
                    None,
                )
                .await
                .map(|out| out.stdout.join(""))?;
            let sum = sum
                .split_ascii_whitespace()
                .next()
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow!("failed to calculate MD5 checksum of source"))?;

            let sources = vec![source_tar_path.to_string_lossy().to_string()];
            let checksums = vec![sum];
            static BUILD_USER: &str = "builduser";

            let pkgbuild = self
                .recipe
                .as_pkgbuild(&image_state.image, &sources, &checksums)
                .render();
            debug!(PKGBUILD = %pkgbuild);

            let entries = vec![("PKGBUILD".to_string(), pkgbuild.as_bytes())];
            let pkgbuild_tar = cloned_span.in_scope(|| create_tar_archive(entries.into_iter()))?;
            let pkgbuild_tar_path = tmp_dir.join("PKGBUILD.tar");

            trace!("copy PKGBUILD archive to container");
            self.container
                .inner()
                .copy_file_into(pkgbuild_tar_path.as_path(), &pkgbuild_tar)
                .await
                .map_err(|e| {
                    anyhow!("failed to copy archive with PKGBUILD to container - {}", e)
                })?;

            trace!("extract PKGBUILD archive");
            self.checked_exec(
                &format!(
                    "tar -xvf {} -C {}",
                    pkgbuild_tar_path.display(),
                    bld_dir.display(),
                ),
                None,
                None,
                None,
            )
            .await?;

            trace!("create build user");
            self.checked_exec(&format!("useradd -m {}", BUILD_USER), None, None, None)
                .await?;
            self.checked_exec(&format!("passwd -d {}", BUILD_USER), None, None, None)
                .await?;
            self.checked_exec(
                &format!("echo -e '{} ALL=(ALL) ALL\n' >> /etc/sudoers", BUILD_USER),
                None,
                None,
                None,
            )
            .await?;
            self.checked_exec(
                &format!("chown -Rv {0}:{0} .", BUILD_USER),
                Some(bld_dir.as_path()),
                None,
                None,
            )
            .await?;
            self.checked_exec("chmod 644 PKGBUILD", Some(bld_dir.as_path()), None, None)
                .await?;

            trace!("makepkg");
            self.checked_exec("makepkg", Some(bld_dir.as_path()), None, Some(BUILD_USER))
                .await
                .map_err(|e| anyhow!("failed to build PKG package - {}", e))?;

            let pkg = format!("{}.pkg.tar.zst", package_name);
            let pkg_path = bld_dir.join(&pkg);

            self.container
                .download_files(&pkg_path, output_dir)
                .await
                .map(|_| output_dir.join(pkg))
                .map_err(|e| anyhow!("failed to download files - {}", e))
        }
        .instrument(span)
        .await
    }
}
