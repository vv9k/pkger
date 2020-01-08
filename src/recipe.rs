use super::*;
#[derive(Deserialize, Debug)]
pub struct Recipe {
    pub info: Info,
    pub env: Option<toml::value::Table>,
    pub build: Build,
    pub install: Install,
    pub finish: Final,
}
impl Recipe {
    pub fn new(entry: DirEntry) -> Result<Recipe, Error> {
        let mut path = entry.path();
        path.push("recipe.toml");
        Ok(toml::from_str::<Recipe>(&fs::read_to_string(&path)?)?)
    }
}
pub type Recipes = HashMap<String, Recipe>;
#[derive(Deserialize, Debug)]
pub struct Info {
    // General
    pub name: String,
    pub version: String,
    pub arch: String,
    pub revision: String,
    pub description: String,
    pub license: String,
    pub source: String,
    pub images: Vec<String>,

    // Git repository as source
    pub git: Option<String>,

    // Debian based specific packages
    pub depends: Option<Vec<String>>,
    pub obsoletes: Option<Vec<String>>,
    pub conflicts: Option<Vec<String>>,
    pub provides: Option<Vec<String>>,

    // RedHat based specific packages
    pub depends_rh: Option<Vec<String>>,
    pub obsoletes_rh: Option<Vec<String>>,
    pub conflicts_rh: Option<Vec<String>>,
    pub provides_rh: Option<Vec<String>>,

    // Directories to exclude when creating the package
    pub exclude: Option<Vec<String>>,

    // Only Debian based
    pub maintainer: Option<String>,
    pub section: Option<String>,
    pub priority: Option<String>,
}
#[derive(Deserialize, Debug)]
pub struct Build {
    pub steps: Vec<String>,
}
#[derive(Deserialize, Debug)]
pub struct Install {
    pub steps: Vec<String>,
}
#[derive(Deserialize, Debug)]
pub struct Final {
    // Final directory where all installed files are
    pub files: String,
    // Path to prepend to all installed files
    pub install_dir: String,
}

const CMD_MLTPL_IMGS: &str = "pkger%:{";
const CMD_MLTPL_IMGS_OFFSET: usize = CMD_MLTPL_IMGS.len();
const CMD_SNGL_IMG: &str = "pkger%:";
const CMD_SNGL_IMG_OFFSET: usize = CMD_SNGL_IMG.len();

#[derive(Debug)]
pub struct Cmd<'a> {
    cmd: String,
    images: Option<Vec<&'a str>>,
}
impl<'a> Cmd<'a> {
    pub fn new(cmd: &'a str) -> Result<Self, Error> {
        trace!("parsing command {}", &cmd);
        // Handle multiple image situation
        if cmd.starts_with(CMD_MLTPL_IMGS) {
            trace!("handling multiple image situation");
            let (images, cmd_idx) = Self::parse_images(&cmd[CMD_MLTPL_IMGS_OFFSET..])?;
            return Ok(Exec {
                cmd: cmd[cmd_idx..].to_string(),
                images: Some(images),
            });
        // Handle single image situation
        } else if cmd.starts_with(CMD_SNGL_IMG) {
            trace!("handling single image situation");
            match cmd.chars().nth(CMD_SNGL_IMG_OFFSET) {
                Some(_ch) => {
                    for (i, ch) in cmd[CMD_SNGL_IMG_OFFSET..].chars().enumerate() {
                        if is_valid_ch(ch) {
                            continue;
                        } else if ch == ' ' {
                            trace!(
                                "found image {}",
                                &cmd[CMD_SNGL_IMG_OFFSET..i + CMD_SNGL_IMG_OFFSET]
                            );
                            return Ok(Exec {
                                cmd: cmd[i + CMD_SNGL_IMG_OFFSET + 1..].to_string(),
                                images: Some(vec![
                                    &cmd[CMD_SNGL_IMG_OFFSET..i + CMD_SNGL_IMG_OFFSET],
                                ]),
                            });
                        } else {
                            return Err(format_err!(
                                "invalid char {} at index {} in command {}",
                                ch,
                                i,
                                &cmd
                            ));
                        }
                    }
                }
                None => return Err(format_err!("command too short: {}", cmd)),
            }
        }
        Ok(Exec {
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
            if is_valid_ch(ch) {
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
                    return Err(format_err!(
                        "invalid format - whitespace is not allowed at this index {} in this cmd {}",
                        i,
                        &cmd
                    ));
                }
            } else if ch == '}' {
                images.push(&cmd[str_start_idx..i]);
                return Ok((images, i + CMD_MLTPL_IMGS_OFFSET + 2));
            } else {
                return Err(format_err!(
                    "invalid character {} at column {} in command {}",
                    ch,
                    i,
                    &cmd
                ));
            }
        }
        Err(format_err!(
            "invalid formatting (missing '}}' perhaps?) in command - {}",
            &cmd
        ))
    }
}
// Checks if character is [a-zA-Z0-9-_]
fn is_valid_ch(ch: char) -> bool {
    if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
        true
    } else {
        false
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_single_image_cmd() {
        let cmd = "pkger%:centos8 echo 'this is a test'";
        let exec = Exec::new(cmd).unwrap();
        assert_eq!(exec.images, Some(vec!["centos8"]));
        assert_eq!(&exec.cmd, "echo 'this is a test'");
    }
    #[test]
    fn parses_multiple_image_cmd_with_whitespace() {
        let cmd = "pkger%:{centos8, debian10, ubuntu18} echo 'this is a test'";
        let exec = Exec::new(cmd).unwrap();
        assert_eq!(exec.images, Some(vec!["centos8", "debian10", "ubuntu18"]));
        assert_eq!(&exec.cmd, "echo 'this is a test'");
    }
    #[test]
    fn parses_multiple_image_cmd_without_whitespace() {
        let cmd = "pkger%:{centos8,debian10,ubuntu18} echo 'this is a test'";
        let exec = Exec::new(cmd).unwrap();
        assert_eq!(exec.images, Some(vec!["centos8", "debian10", "ubuntu18"]));
        assert_eq!(&exec.cmd, "echo 'this is a test'");
    }
    #[test]
    fn parses_normal_cmd() {
        let cmd = "echo 'this is a test' || exit 1";
        let exec = Exec::new(cmd).unwrap();
        assert_eq!(exec.images, None);
        assert_eq!(&exec.cmd, "echo 'this is a test' || exit 1");
    }
}
