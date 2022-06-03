use crate::build::container::Context;
use crate::container::ExecOpts;
use crate::log::{info, trace, BoxedCollector};
use crate::recipe::GitSource;
use crate::template;
use crate::Result;

use std::fs;
use std::path::{Path, PathBuf};

pub async fn fetch_git_source(
    ctx: &Context<'_>,
    repo: &GitSource,
    logger: &mut BoxedCollector,
) -> Result<()> {
    info!(logger => "cloning git repository to {}, url = {}, branch = {}", ctx.build.container_bld_dir.display(),repo.url(), repo.branch());
    ctx.checked_exec(
        &ExecOpts::default().cmd(&format!(
            "git clone -j 8 --single-branch --branch {} --recurse-submodules -- {} {}",
            repo.branch(),
            repo.url(),
            ctx.build.container_bld_dir.display()
        )),
        logger,
    )
    .await
    .map(|_| ())
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

    let mut entries = Vec::new();
    for f in files {
        trace!(logger => "adding file {} to archive", f.display());
        let filename = f
            .file_name()
            .map(|s| PathBuf::from(format!("./{}", s.to_string_lossy())))
            .unwrap_or_default();
        entries.push((filename, fs::read(f)?));
    }

    ctx.container
        .upload_files(
            entries.iter().map(|(p, b)| (p.as_path(), &b[..])).collect(),
            dest,
            logger,
        )
        .await?;

    Ok(())
}

pub async fn fetch_source(ctx: &Context<'_>, logger: &mut BoxedCollector) -> Result<()> {
    if let Some(repo) = &ctx.build.recipe.metadata.git {
        fetch_git_source(ctx, repo, logger).await?;
    } else if let Some(source) = &ctx.build.recipe.metadata.source {
        let source = template::render(source, ctx.vars.inner());
        if source.starts_with("http") {
            fetch_http_source(ctx, source.as_str(), &ctx.build.container_tmp_dir, logger).await?;
        } else {
            let src_path = PathBuf::from(source);
            fetch_fs_source(
                ctx,
                &[src_path.as_path()],
                &ctx.build.container_tmp_dir,
                logger,
            )
            .await?;
        }
        ctx.checked_exec(
            &ExecOpts::default()
                .cmd(&format!(
                    r#"
                        for file in *;
                        do
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
    }
    Ok(())
}
