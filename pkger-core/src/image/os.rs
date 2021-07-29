use crate::docker::{api::ContainerCreateOpts, Docker};
use crate::oneshot::{self, OneShotCtx};
use crate::recipe::Os;
use crate::{ErrContext, Error, Result};

use tracing::{info_span, trace, Instrument};

/// Finds out the operating system and version of the image with id `image_id`
pub async fn find(image_id: &str, docker: &Docker) -> Result<Os> {
    let span = info_span!("find-os");
    macro_rules! return_if_ok {
        ($check:expr) => {
            match $check
                .instrument(span.clone())
                .await
            {
                Ok(os) => return Ok(os),
                Err(e) => trace!(reason = %e),
            }
        };
    }

    return_if_ok!(from_osrelease(image_id, docker));
    return_if_ok!(from_issue(image_id, docker));
    return_if_ok!(from_rhrelease(image_id, docker));

    Err(Error::msg("failed to determine distribution"))
}

async fn from_osrelease(image_id: &str, docker: &Docker) -> Result<Os> {
    let out = oneshot::run(&OneShotCtx::new(
        docker,
        &ContainerCreateOpts::builder(&image_id)
            .cmd(vec!["cat", "/etc/os-release"])
            .build(),
        true,
        true,
    ))
    .await?;

    trace!(stderr = %String::from_utf8_lossy(&out.stderr));

    let out = String::from_utf8_lossy(&out.stdout);
    trace!(stdout = %out);

    fn extract_key(out: &str, key: &str) -> Option<String> {
        let key = [key, "="].join("");
        if let Some(line) = out.lines().find(|line| line.starts_with(&key)) {
            let line = line.strip_prefix(&key).unwrap();
            if line.starts_with('"') {
                return Some(line.trim_matches('"').to_string());
            }
            return Some(line.to_string());
        }
        None
    }

    let os_name = extract_key(&out, "ID");
    let version = extract_key(&out, "VERSION_ID");
    Os::new(os_name.context("os name is missing")?, version)
}

fn extract_version(text: &str) -> Option<String> {
    let mut chars = text.chars();
    if let Some(idx) = chars.position(|c| c.is_numeric()) {
        let mut end_idx = idx;
        for ch in chars {
            let is_valid = ch.is_numeric() || ch == '.' || ch == '-';
            if !is_valid {
                break;
            }
            end_idx += 1;
        }
        Some(text[idx..=end_idx].to_string())
    } else {
        None
    }
}

async fn os_from(image_id: &str, docker: &Docker, file: &str) -> Result<Os> {
    let out = oneshot::run(&OneShotCtx::new(
        docker,
        &ContainerCreateOpts::builder(&image_id)
            .cmd(vec!["cat", file])
            .build(),
        true,
        true,
    ))
    .await?;

    trace!(stderr = %String::from_utf8_lossy(&out.stderr));

    let out = String::from_utf8_lossy(&out.stdout);
    trace!(stdout = %out);

    let os_version = extract_version(&out);

    Os::new(out, os_version)
}

async fn from_rhrelease(image_id: &str, docker: &Docker) -> Result<Os> {
    os_from(image_id, docker, "/etc/redhat-release").await
}

async fn from_issue(image_id: &str, docker: &Docker) -> Result<Os> {
    os_from(image_id, docker, "/etc/issue").await
}
