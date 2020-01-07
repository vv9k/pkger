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

pub struct Exec<'a> {
    cmd: String,
    images: Option<Vec<&'a str>>,
}
impl<'a> Exec<'a> {
    fn new(cmd: &'a str) -> Result<Self, Error> {
        trace!("parsing command {}", &cmd);
        // Handle multiple image situation
        if cmd.starts_with("pkger%:{") {
            let (images, cmd_idx) = Self::parse_images(cmd)?;
            return Ok(Exec {
                cmd: cmd[cmd_idx..].to_string(),
                images: Some(images),
            });
        // Handle single image situation
        } else if cmd.starts_with("pkger%:") {
            match cmd.chars().nth(7) {
                Some(_ch) => {
                    let mut image_name = String::new();

                    for (i, ch) in cmd[7..].chars().enumerate() {
                        if is_valid_ch(ch) {
                            image_name.push(ch);
                        } else if ch == ' ' {
                            return Ok(Exec {
                                cmd: cmd[i + 1..].to_string(),
                                images: Some(vec![&cmd[7..i - 1]]),
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
        trace!("parsing image names from cmd {}", &cmd);
        // Handle multiple images situation
        let mut images = Vec::new();
        let mut str_start_idx = 0;
        // Allow whitespace only after ','
        let mut sep = false;
        for (i, ch) in cmd.chars().enumerate() {
            if is_valid_ch(ch) {
                continue;
            } else if ch == ',' {
                // The flag had to be false otherwise the cmd is invalid
                assert!(!sep);
                sep = true;
                images.push(&cmd[str_start_idx..i - 1]);
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
                images.push(&cmd[str_start_idx..i - 1]);
                return Ok((images, i + 2));
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
