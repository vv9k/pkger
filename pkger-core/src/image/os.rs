use crate::container::CreateOpts;
use crate::docker::Docker;
use crate::log::{info, trace, BoxedCollector};
use crate::oneshot::{self, OneShotCtx};
use crate::recipe::Os;
use crate::{err, ErrContext, Error, Result};

/// Finds out the operating system and version of the image with id `image_id`
pub async fn find(image_id: &str, docker: &Docker, logger: &mut BoxedCollector) -> Result<Os> {
    info!(logger => "finding os of image {}", image_id);

    macro_rules! return_if_ok {
        ($check:expr) => {
            match $check
                .await
            {
                Ok(os) => return Ok(os),
                Err(e) => trace!(logger => "{:?}", e),
            }
        };
    }

    return_if_ok!(from_osrelease(image_id, docker, logger));
    return_if_ok!(from_issue(image_id, docker, logger));
    return_if_ok!(from_rhrelease(image_id, docker, logger));

    err!("failed to determine distribution")
}

async fn from_osrelease(
    image_id: &str,
    docker: &Docker,
    logger: &mut BoxedCollector,
) -> Result<Os> {
    let out = oneshot::run(
        &OneShotCtx::new(
            docker,
            &CreateOpts::new(image_id).cmd(vec!["cat", "/etc/os-release"]),
            true,
            true,
        ),
        logger,
    )
    .await?;

    trace!(logger => "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let out = String::from_utf8_lossy(&out.stdout);
    trace!(logger => "stdout: {}", out);

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

async fn os_from(
    image_id: &str,
    docker: &Docker,
    file: &str,
    logger: &mut BoxedCollector,
) -> Result<Os> {
    let out = oneshot::run(
        &OneShotCtx::new(
            docker,
            &CreateOpts::new(image_id).cmd(vec!["cat", file]),
            true,
            true,
        ),
        logger,
    )
    .await?;

    trace!(logger => "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let out = String::from_utf8_lossy(&out.stdout);
    trace!(logger => "stdout: {}", out);

    let os_version = extract_version(&out);

    Os::new(out, os_version)
}

async fn from_rhrelease(
    image_id: &str,
    docker: &Docker,
    logger: &mut BoxedCollector,
) -> Result<Os> {
    os_from(image_id, docker, "/etc/redhat-release", logger).await
}

async fn from_issue(image_id: &str, docker: &Docker, logger: &mut BoxedCollector) -> Result<Os> {
    os_from(image_id, docker, "/etc/issue", logger).await
}
