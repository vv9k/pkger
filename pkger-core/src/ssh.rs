use crate::{ErrContext, Error, Result};

use std::env;
use std::path::PathBuf;

pub const SOCK_ENV: &str = "SSH_AUTH_SOCK";

/// Returns the path to the SSH authentication socket depending on the operating system
/// and checks if the socket exists.
pub fn auth_sock() -> Result<String> {
    #[cfg(target_os = "linux")]
    let socket = env::var(SOCK_ENV).context("missing ssh auth socket environment variable")?;

    #[cfg(target_os = "macos")]
    let socket = "/run/host-services/ssh-auth.sock".to_owned();

    let path = PathBuf::from(&socket);
    if !path.exists() {
        return Err(Error::msg("ssh auth socket does not exist"));
    }

    Ok(socket)
}
