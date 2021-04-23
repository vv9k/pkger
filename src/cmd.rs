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

    macro_rules! test_cmd {
        (
            input = $cmd:expr,
            want = $cmd_out:expr,
            images = $($image:expr),*) => {
            let expect_images: Vec<&str> = vec![ $($image),* ];
            let cmd = Cmd::new($cmd).unwrap();
            assert_eq!(expect_images, cmd.images);
            assert_eq!($cmd_out, &cmd.cmd);
        }
    }

    #[test]
    #[rustfmt::skip]
    fn parses_cmd() {
        test_cmd!(
            input  = "echo 'normal cmd'",
            want   = "echo 'normal cmd'",
            images =
        );
        test_cmd!(
            input  = "pkger%:{centos8,debian10,ubuntu18} echo 'multiple images'",
            want   = "echo 'multiple images'",
            images = "centos8", "debian10", "ubuntu18"
        );
        test_cmd!(
            input  = "pkger%:centos8 echo 'single image'",
            want   = "echo 'single image'",
            images = "centos8"
        );
        test_cmd!(
            input  = "pkger%:{centos8, debian10, ubuntu18} echo 'normal whitespace'",
            want   = "echo 'normal whitespace'",
            images = "centos8", "debian10", "ubuntu18"
        );
        test_cmd!(
            input  = "pkger%:{ centos8, debian10, ubuntu18} echo 'left padded'",
            want   = "echo 'left padded'",
            images = "centos8", "debian10", "ubuntu18"
        );
        test_cmd!(
            input  = "pkger%:{ centos8, debian10, ubuntu18 } echo 'all sides padded'",
            want   = "echo 'all sides padded'",
            images = "centos8", "debian10", "ubuntu18"
        );
        test_cmd!(
            input  = "pkger%:{ centos8,debian10,ubuntu18 } echo 'both sides padded'",
            want   = "echo 'both sides padded'",
            images = "centos8", "debian10", "ubuntu18"
        );
    }
}
