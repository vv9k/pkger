use crate::build::container::Context;
use crate::container::ExecOpts;
use crate::log::{info, trace, BoxedCollector};
use crate::proxy::ShouldProxyResult;
use crate::recipe::GitSource;
use crate::template;
use crate::{unix_timestamp, ErrContext, Result};

use std::path::{Path, PathBuf};

pub async fn fetch_git_source(
    ctx: &Context<'_>,
    repo: &GitSource,
    logger: &mut BoxedCollector,
) -> Result<()> {
    info!(logger => "cloning git repository to {}, url = {}, branch = {}", ctx.build.container_bld_dir.display(),repo.url(), repo.branch());

    let tmp = tempdir::TempDir::new(&ctx.build.id)
        .context("failed to initialize temporary directory for git repo")?;

    tokio::task::block_in_place(|| {
        let mut repo_builder = git2::build::RepoBuilder::new();

        let mut proxy_opts = git2::ProxyOptions::new();

        match ctx.build.proxy.should_proxy(repo.url()) {
            ShouldProxyResult::Http => {
                if let Some(url) = ctx.build.proxy.http_proxy() {
                    proxy_opts.url(&url.to_string());
                }
            }
            ShouldProxyResult::Https => {
                if let Some(url) = ctx.build.proxy.https_proxy() {
                    proxy_opts.url(&url.to_string());
                }
            }
            _ => {}
        }

        let mut opts = git2::FetchOptions::new();
        opts.proxy_options(proxy_opts);

        repo_builder.branch(repo.branch());
        repo_builder.fetch_options(opts);
        repo_builder
            .clone(repo.url(), tmp.path())
            .context("failed to clone git repository")
    })?;

    let tar_file = vec![];
    let mut tar = tar::Builder::new(tar_file);

    tar.append_dir_all("./", tmp.path())
        .context("failed to build tar archive of git repo")?;
    tar.finish()?;
    let tar_file = tar.into_inner()?;
    let tar_name = format!("git-repo-{}.tar", unix_timestamp().as_secs());

    ctx.container
        .upload_and_extract_archive(tar_file, &ctx.build.container_bld_dir, &tar_name, logger)
        .await
        .context("failed to upload git repo")
}

pub async fn fetch_http_source(
    ctx: &Context<'_>,
    source: &str,
    dest: &Path,
    logger: &mut BoxedCollector,
) -> Result<()> {
    info!(logger => "fetching http source to {}, url = {}", dest.display(), source);

    ctx.checked_exec(
        &ExecOpts::default()
            .cmd(&format!("curl -LO {}", source))
            .working_dir(dest),
        logger,
    )
    .await
    .map(|_| ())
}

pub async fn fetch_fs_source(
    ctx: &Context<'_>,
    files: &[&Path],
    dest: &Path,
    logger: &mut BoxedCollector,
) -> Result<()> {
    info!(logger => "fetching files to {}", dest.display());

    let tar_file = vec![];
    let mut tar = tar::Builder::new(tar_file);

    for path in files {
        if path.is_dir() {
            trace!(logger => "adding entry {} to archive", path.display());
            let dir_name = path.file_name().unwrap_or_default();
            tar.append_dir_all(format!("./{}", dir_name.to_string_lossy()), path)
                .context("failed adding directory to archive")?;
        } else if path.is_file() {
            trace!(logger => "adding file {} to archive", path.display());
            let mut file = std::fs::File::open(path).context("failed to open file to add to archive")?;
            let file_name = path.file_name().unwrap_or_default();
            tar.append_file(&format!("./{}", file_name.to_string_lossy()), &mut file)
                .context("failed adding file to archive")?;
        }
    }

    tar.finish()?;
    let tar_file = tar.into_inner()?;

    let tar_name = format!("fs-source-{}.tar", unix_timestamp().as_secs());

    ctx.container
        .upload_and_extract_archive(tar_file, &dest, &tar_name, logger)
        .await
}

pub async fn fetch_source(ctx: &Context<'_>, logger: &mut BoxedCollector) -> Result<()> {
    if let Some(repo) = &ctx.build.recipe.metadata.git {
        fetch_git_source(ctx, repo, logger).await?;
    } else if !ctx.build.recipe.metadata.source.is_empty() {
        for source in &ctx.build.recipe.metadata.source {
            if source.starts_with("http") {
                fetch_http_source(ctx, &source, &ctx.build.container_tmp_dir, logger).await?;
            } else {
                let p = PathBuf::from(source);
                let source = if p.is_absolute() {
                    p
                } else {
                    ctx.build
                        .recipe_dir
                        .join(&ctx.build.recipe.metadata.name)
                        .join(template::render(source, ctx.vars.inner()))
                };
                fetch_fs_source(
                    ctx,
                    &[source.as_path()],
                    &ctx.build.container_bld_dir,
                    logger,
                )
                .await?;
            }
        }
        ctx.checked_exec(
            &ExecOpts::default()
                .cmd(&format!(
                    r#"
                        for file in *;
                        do
                            [ -f "$file" ] || continue
                            if [[ $file =~ (.*[.]tar.*|.*[.](tgz|tbz|txz|tlz|tsz|taz|tz)) ]]
                            then
                                tar xvf $file -C {0}
                            elif [[ $file == *.zip ]]
                            then
                                unzip $file -d {0}
                            else
                                cp -v $file {0}
                            fi
                        done"#,
                    ctx.build.container_bld_dir.display(),
                ))
                .working_dir(&ctx.build.container_tmp_dir)
                .shell("/bin/bash"),
            logger,
        )
        .await?;
    } else {
        trace!(logger => "no sources to fetch");
    }
    Ok(())
}
