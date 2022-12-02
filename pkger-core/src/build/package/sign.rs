use crate::build::container::Context;
use crate::log::{info, BoxedCollector};
use crate::runtime::container::ExecOpts;
use crate::{ErrContext, Result};

use crate::gpg::GpgKey;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Uploads the `gpg_key` to `destination` in the container and returns the
/// full path of the key in the container.
pub(crate) async fn upload_gpg_key(
    ctx: &Context<'_>,
    gpg_key: &GpgKey,
    destination: &Path,
    logger: &mut BoxedCollector,
) -> Result<PathBuf> {
    info!(logger => "uploading GPG key to '{}'", destination.display());
    let key = fs::read(gpg_key.path()).context("failed reading the gpg key")?;

    ctx.container
        .upload_files(
            vec![(PathBuf::from("./GPG-SIGN-KEY").as_path(), key.as_slice())],
            destination,
            logger,
        )
        .await
        .map(|_| destination.join("GPG-SIGN-KEY"))
        .context("failed to upload gpg key")
}

/// Imports the gpg key located at `path` to the database in the container.
pub(crate) async fn import_gpg_key(
    ctx: &Context<'_>,
    gpg_key: &GpgKey,
    path: &Path,
    logger: &mut BoxedCollector,
) -> Result<()> {
    info!(logger => "importing GPG key from '{}'", path.display());
    ctx.checked_exec(
        &ExecOpts::new().cmd(&format!(
            r#"gpg --pinentry-mode=loopback --passphrase {} --import {}"#,
            gpg_key.pass(),
            path.display(),
        )),
        logger,
    )
    .await
    .map(|_| ())
}
