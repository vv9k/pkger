use crate::job::build::BuildContainerCtx;
use crate::recipe::GitSource;
use crate::util::create_tar_archive;
use crate::Result;

use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, info_span, Instrument};

impl<'job> BuildContainerCtx<'job> {
    pub async fn archive_output_dir(&self) -> Result<Vec<u8>> {
        let span = info_span!("archive-output", container_dir = %self.container_out_dir.display());
        async move {
            info!("copying final archive");
            self.container.copy_from(self.container_out_dir).await
        }
        .instrument(span)
        .await
    }

    pub async fn clone_git_to_bld_dir(&self, repo: &GitSource) -> Result<()> {
        let span = info_span!("clone-git");
        async move {
                info!(repo = %repo.url(), branch = %repo.branch(), out_dir = %self.container_bld_dir.display(), "cloning git source repository to build directory");
                self.checked_exec(&format!(
                    "git clone --single-branch --branch {} --recurse-submodules -- {} {}",
                    repo.branch(),
                    repo.url(),
                    self.container_bld_dir.display()
                ), None, None)
                .await
                .map(|_| ())
        }
        .instrument(span)
        .await
    }

    pub async fn get_http_source(&self, source: &str, dest: &Path) -> Result<()> {
        let span = info_span!("download-http");
        async move {
            info!(url = %source, destination = %dest.display(), "fetching");
            self.checked_exec(&format!("curl -LO {}", source), Some(dest), None)
                .await
                .map(|_| ())
        }
        .instrument(span)
        .await
    }

    pub async fn copy_files_into(&self, files: &[&Path], dest: &Path) -> Result<()> {
        let span = info_span!("copy-files-into");
        let mut entries = Vec::new();
        for f in files {
            debug!(parent: &span, entry = %f.display(), "adding");
            entries.push((*f, fs::read(f)?));
        }

        let archive =
            span.in_scope(|| create_tar_archive(entries.iter().map(|(p, b)| (*p, &b[..]))))?;

        self.container
            .inner()
            .copy_file_into(dest, &archive)
            .instrument(span.clone())
            .await?;

        Ok(())
    }

    pub async fn fetch_source(&self) -> Result<()> {
        let span = info_span!("fetch");
        async move {
            if let Some(repo) = &self.recipe.metadata.git {
                self.clone_git_to_bld_dir(repo).await?;
            } else if let Some(source) = &self.recipe.metadata.source {
                if source.starts_with("http") {
                    self.get_http_source(source.as_str(), self.container_tmp_dir)
                        .await?;
                } else {
                    let src_path = PathBuf::from(source);
                    self.copy_files_into(&[src_path.as_path()], self.container_tmp_dir)
                        .await?;
                }
                self.checked_exec(
                    &format!(
                        r#"
                        for file in *;
                        do
                            if [[ \$file == *.tar* ]]
                            then
                                tar xvf \$file -C {0}
                            elif [[ \$file == *.zip ]]
                            then
                                unzip -v \$file -d {0}
                            else
                                cp -v \$file {0}
                            fi
                        done"#,
                        self.container_bld_dir.display(),
                    ),
                    Some(self.container_tmp_dir),
                    Some("/bin/bash"),
                )
                .await?;
            }
            Ok(())
        }
        .instrument(span)
        .await
    }
}