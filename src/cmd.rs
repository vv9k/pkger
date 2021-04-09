use crate::Error;
use anyhow::anyhow;

const CMD_MLTPL_IMGS: &str = "pkger%:{";
const CMD_MLTPL_IMGS_OFFSET: usize = CMD_MLTPL_IMGS.len();
const CMD_SNGL_IMG: &str = "pkger%:";
const CMD_SNGL_IMG_OFFSET: usize = CMD_SNGL_IMG.len();

#[derive(Debug)]
pub struct Cmd<'a> {
    pub cmd: String,
    pub images: Option<Vec<&'a str>>,
}
impl<'a> Cmd<'a> {
    pub fn new(cmd: &'a str) -> Result<Self, Error> {
        // Handle multiple image situation
        if cmd.starts_with(CMD_MLTPL_IMGS) {
            let (images, cmd_idx) = Self::parse_images(&cmd[CMD_MLTPL_IMGS_OFFSET..])?;
            return Ok(Cmd {
                cmd: cmd[cmd_idx..].to_string(),
                images: Some(images),
            });
        // Handle single image situation
        } else if cmd.starts_with(CMD_SNGL_IMG) {
            match cmd.chars().nth(CMD_SNGL_IMG_OFFSET) {
                Some(_ch) => {
                    for (i, ch) in cmd[CMD_SNGL_IMG_OFFSET..].chars().enumerate() {
                        if is_valid_name_ch(ch) {
                            continue;
                        } else if ch == ' ' {
                            return Ok(Cmd {
                                cmd: cmd[i + CMD_SNGL_IMG_OFFSET + 1..].to_string(),
                                images: Some(vec![
                                    &cmd[CMD_SNGL_IMG_OFFSET..i + CMD_SNGL_IMG_OFFSET],
                                ]),
                            });
                        } else {
                            return Err(anyhow!(
                                "invalid char {} at index {} in command {}",
                                ch,
                                i,
                                &cmd
                            ));
                        }
                    }
                }
                None => return Err(anyhow!("command too short: {}", cmd)),
            }
        }
        Ok(Cmd {
            cmd: cmd.to_string(),
            images: None,
        })
    }
    fn parse_images(cmd: &str) -> Result<(Vec<&str>, usize), Error> {
        let mut images = Vec::new();
        let mut str_start_idx = 0;
        // Allow whitespace only after ','
        let mut sep = false;
        for (i, ch) in cmd.chars().enumerate() {
            if is_valid_name_ch(ch) {
                continue;
            } else if ch == ',' {
                sep = true;
                images.push(&cmd[str_start_idx..i]);
                str_start_idx = i + 1;
            } else if ch == ' ' {
                if sep {
                    str_start_idx = i + 1;
                    sep = false;
                } else {
                    return Err(anyhow!(
                        "invalid format - whitespace is not allowed at this index {} in this cmd {}",
                        i,
                        &cmd
                    ));
                }
            } else if ch == '}' {
                images.push(&cmd[str_start_idx..i]);
                return Ok((images, i + CMD_MLTPL_IMGS_OFFSET + 2));
            } else {
                return Err(anyhow!(
                    "invalid character {} at column {} in command {}",
                    ch,
                    i,
                    &cmd
                ));
            }
        }
        Err(anyhow!(
            "invalid formatting (missing '}}' perhaps?) in command - {}",
            &cmd
        ))
    }
}
// Checks if character is [a-zA-Z0-9-_]
fn is_valid_name_ch(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}
#[cfg(test)]
mod command {
    use super::*;
    #[test]
    fn parses_single_image_cmd() {
        let cmd = "pkger%:centos8 echo 'this is a test'";
        let exec = Cmd::new(cmd).unwrap();
        assert_eq!(exec.images, Some(vec!["centos8"]));
        assert_eq!(&exec.cmd, "echo 'this is a test'");
    }
    #[test]
    fn parses_multiple_image_cmd_with_whitespace() {
        let cmd = "pkger%:{centos8, debian10, ubuntu18} echo 'this is a test'";
        let exec = Cmd::new(cmd).unwrap();
        assert_eq!(exec.images, Some(vec!["centos8", "debian10", "ubuntu18"]));
        assert_eq!(&exec.cmd, "echo 'this is a test'");
    }
    #[test]
    fn parses_multiple_image_cmd_without_whitespace() {
        let cmd = "pkger%:{centos8,debian10,ubuntu18} echo 'this is a test'";
        let exec = Cmd::new(cmd).unwrap();
        assert_eq!(exec.images, Some(vec!["centos8", "debian10", "ubuntu18"]));
        assert_eq!(&exec.cmd, "echo 'this is a test'");
    }
    #[test]
    fn parses_normal_cmd() {
        let cmd = "echo 'this is a test' || exit 1";
        let exec = Cmd::new(cmd).unwrap();
        assert_eq!(exec.images, None);
        assert_eq!(&exec.cmd, "echo 'this is a test' || exit 1");
    }
}
