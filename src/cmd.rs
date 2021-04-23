use crate::Result;

use anyhow::anyhow;

const CMD_PREFIX: &str = "pkger%:";

#[derive(Clone, Debug)]
/// Wrapper type for steps parsed from a recipe. Is used to easily distinguish on which images the
/// commands should be executed.
pub struct Cmd {
    pub cmd: String,
    pub images: Vec<String>,
}

impl Cmd {
    /// Parses a command from a string. If a command begins with [`CMD_PREFIX`](CMD_PREFIX) the images
    /// that follow the prefix will be added to `images` field of `Cmd`.
    ///
    /// Some examples:
    ///  'pkger%:centos8 echo test'  => execute only on centos8
    ///  'pkger%:{centos8, debian10} echo test'  => execute only on centos8 and debian10
    ///  'echo test'  => execute on all
    pub fn new(cmd: &str) -> Result<Self> {
        if let Some(cmd) = cmd.strip_prefix(CMD_PREFIX) {
            Self::parse_prefixed_command(cmd)
        } else {
            Ok(Self::parse_simple_command(cmd))
        }
    }

    fn parse_simple_command(cmd: &str) -> Self {
        Cmd {
            cmd: cmd.to_string(),
            images: vec![],
        }
    }

    fn parse_prefixed_command(cmd: &str) -> Result<Self> {
        if let Some(cmd) = cmd.strip_prefix('{') {
            Self::parse_multiple_images(cmd)
        } else {
            Self::parse_single_image(cmd)
        }
    }

    fn parse_multiple_images(cmd: &str) -> Result<Self> {
        if let Some(end) = cmd.find('}') {
            Ok(Cmd {
                cmd: cmd[end + 1..].trim_start().to_string(),
                images: cmd[..end]
                    .split(',')
                    .into_iter()
                    .map(|image| image.trim().to_string())
                    .collect::<Vec<_>>(),
            })
        } else {
            Err(anyhow!("missing ending `}}` in `{}`", cmd))
        }
    }

    fn parse_single_image(cmd: &str) -> Result<Self> {
        if let Some(end) = cmd.find(' ') {
            let image = &cmd[..end];
            for ch in image.chars() {
                if !is_valid_name_ch(ch) {
                    return Err(anyhow!("invalid char in name `{}`", ch));
                }
            }
            Ok(Cmd {
                cmd: cmd[end..].trim_start().to_string(),
                images: vec![cmd[..end].to_string()],
            })
        } else {
            Err(anyhow!("missing whitespace after image name in `{}`", cmd))
        }
    }
}

// Checks if character is [a-zA-Z0-9-_]
const fn is_valid_name_ch(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

#[cfg(test)]
mod command {
    use super::*;
    #[test]
    fn parses_single_image_cmd() {
        let cmd = "pkger%:centos8 echo 'this is a test'";
        let exec = Cmd::new(cmd).unwrap();
        assert_eq!(exec.images, vec!["centos8"]);
        assert_eq!(&exec.cmd, "echo 'this is a test'");
    }
    #[test]
    fn parses_multiple_image_cmd_with_whitespace() {
        let mut cmd = "pkger%:{centos8, debian10, ubuntu18} echo 'this is a test'";
        let mut exec = Cmd::new(cmd).unwrap();
        assert_eq!(exec.images, vec!["centos8", "debian10", "ubuntu18"]);
        assert_eq!(&exec.cmd, "echo 'this is a test'");

        cmd = "pkger%:{ centos8, debian10, ubuntu18} echo 'this is a test'";
        exec = Cmd::new(cmd).unwrap();
        assert_eq!(exec.images, vec!["centos8", "debian10", "ubuntu18"]);
        assert_eq!(&exec.cmd, "echo 'this is a test'");

        cmd = "pkger%:{ centos8, debian10, ubuntu18 } echo 'this is a test'";
        exec = Cmd::new(cmd).unwrap();
        assert_eq!(exec.images, vec!["centos8", "debian10", "ubuntu18"]);
        assert_eq!(&exec.cmd, "echo 'this is a test'");

        cmd = "pkger%:{ centos8,debian10, ubuntu18 } echo 'this is a test'";
        exec = Cmd::new(cmd).unwrap();
        assert_eq!(exec.images, vec!["centos8", "debian10", "ubuntu18"]);
        assert_eq!(&exec.cmd, "echo 'this is a test'");

        cmd = "pkger%:{ centos8,debian10,ubuntu18 } echo 'this is a test'";
        exec = Cmd::new(cmd).unwrap();
        assert_eq!(exec.images, vec!["centos8", "debian10", "ubuntu18"]);
        assert_eq!(&exec.cmd, "echo 'this is a test'");
    }
    #[test]
    fn parses_multiple_image_cmd_without_whitespace() {
        let cmd = "pkger%:{centos8,debian10,ubuntu18} echo 'this is a test'";
        let exec = Cmd::new(cmd).unwrap();
        assert_eq!(exec.images, vec!["centos8", "debian10", "ubuntu18"]);
        assert_eq!(&exec.cmd, "echo 'this is a test'");
    }
    #[test]
    fn parses_normal_cmd() {
        let cmd = "echo 'this is a test' || exit 1";
        let exec = Cmd::new(cmd).unwrap();
        assert_eq!(exec.images, Vec::<&str>::new());
        assert_eq!(&exec.cmd, "echo 'this is a test' || exit 1");
    }
}
