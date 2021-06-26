use crate::archive::create_tarball;
use crate::build::{container::checked_exec, BuildContainerCtx};
use crate::container::ExecOpts;
use crate::recipe::GitSource;
use crate::Result;

use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, info_span, Instrument};

pub async fn clone_git_to_bld_dir(ctx: &BuildContainerCtx<'_>, repo: &GitSource) -> Result<()> {
    let span = info_span!("clone-git");
    async move {
                info!(repo = %repo.url(), branch = %repo.branch(), out_dir = %ctx.container_bld_dir.display(), "cloning git source repository to build directory");
                checked_exec(
                    &ctx,
                    &ExecOpts::default().cmd(
                    &format!(
                    "git clone --single-branch --branch {} --recurse-submodules -- {} {}",
                    repo.branch(),
                    repo.url(),
                    ctx.container_bld_dir.display()
                )).build())
                .await
                .map(|_| ())
        }
        .instrument(span)
        .await
}

pub async fn get_http_source(ctx: &BuildContainerCtx<'_>, source: &str, dest: &Path) -> Result<()> {
    let span = info_span!("download-http");
    async move {
        info!(url = %source, destination = %dest.display(), "fetching");
        checked_exec(
            &ctx,
            &ExecOpts::default()
                .cmd(&format!("curl -LO {}", source))
                .working_dir(dest)
                .build(),
        )
        .await
        .map(|_| ())
    }
    .instrument(span)
    .await
}

pub async fn copy_files_into(
    ctx: &BuildContainerCtx<'_>,
    files: &[&Path],
    dest: &Path,
) -> Result<()> {
    let span = info_span!("copy-files-into");
    let mut entries = Vec::new();
    for f in files {
        debug!(parent: &span, entry = %f.display(), "adding");
        let filename = f
            .file_name()
            .map(|s| format!("./{}", s.to_string_lossy()))
            .unwrap_or_default();
        entries.push((filename, fs::read(f)?));
    }

    let archive = span.in_scope(|| create_tarball(entries.iter().map(|(p, b)| (p, &b[..]))))?;

    ctx.container
        .inner()
        .copy_file_into(dest, &archive)
        .instrument(span.clone())
        .await?;

    Ok(())
}

pub async fn fetch_source(ctx: &BuildContainerCtx<'_>) -> Result<()> {
    let span = info_span!("fetch");
    async move {
        if let Some(repo) = &ctx.recipe.metadata.git {
            clone_git_to_bld_dir(ctx, repo).await?;
        } else if let Some(source) = &ctx.recipe.metadata.source {
            if source.starts_with("http") {
                get_http_source(ctx, source.as_str(), ctx.container_tmp_dir).await?;
            } else {
                let src_path = PathBuf::from(source);
                copy_files_into(ctx, &[src_path.as_path()], ctx.container_tmp_dir).await?;
            }
            checked_exec(
                ctx,
                &ExecOpts::default()
                    .cmd(&format!(
                        r#"
                        for file in *;
                        do
                            if [[ $file == *.tar* ]]
                            then
                                tar xvf $file -C {0}
                            elif [[ $file == *.zip ]]
                            then
                                unzip $file -d {0}
                            else
                                cp -v $file {0}
                            fi
                        done"#,
                        ctx.container_bld_dir.display(),
                    ))
                    .working_dir(ctx.container_tmp_dir)
                    .shell("/bin/bash")
                    .build(),
            )
            .await?;
        }
        Ok(())
    }
    .instrument(span)
    .await
}
