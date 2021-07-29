use crate::build::container::{checked_exec, Context};
use crate::container::ExecOpts;
use crate::{ErrContext, Result};

use crate::gpg::GpgKey;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tracing::{info_span, Instrument};

/// Uploads the `gpg_key` to `destination` in the container and returns the
/// full path of the key in the container.
pub(crate) async fn upload_gpg_key(
    ctx: &Context<'_>,
    gpg_key: &GpgKey,
    destination: &Path,
) -> Result<PathBuf> {
    let span = info_span!("upload-key", path = %destination.display());
    let key = span
        .clone()
        .in_scope(|| fs::read(&gpg_key.path()))
        .context("failed reading the gpg key")?;

    ctx.container
        .upload_files(
            vec![("./GPG-SIGN-KEY", key.as_slice())],
            &destination,
            ctx.build.quiet,
        )
        .instrument(span)
        .await
        .map(|_| destination.join("GPG-SIGN-KEY"))
        .context("failed to upload gpg key")
}

/// Imports the gpg key located at `path` to the database in the container.
pub(crate) async fn import_gpg_key(ctx: &Context<'_>, gpg_key: &GpgKey, path: &Path) -> Result<()> {
    let span = info_span!("import-key", path = %path.display());
    checked_exec(
        ctx,
        &ExecOpts::default()
            .cmd(&format!(
                r#"gpg --pinentry-mode=loopback --passphrase {} --import {}"#,
                gpg_key.pass(),
                path.display(),
            ))
            .build(),
    )
    .instrument(span)
    .await
    .map(|_| ())
}
