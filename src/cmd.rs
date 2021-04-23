use crate::Result;

use std::convert::TryFrom;

#[derive(Clone, Debug)]
/// Wrapper type for steps parsed from a recipe. Can be either a simple string or a map specifying
/// other parameters.
///
/// Examples:
/// "echo 123"
///
/// { cmd = "echo 123", images = ["centos8", "debian10"] }
pub struct Cmd {
    pub cmd: String,
    pub images: Vec<String>,
}

impl TryFrom<&toml::Value> for Cmd {
    type Error = crate::Error;

    fn try_from(value: &toml::Value) -> Result<Self> {
        match value {
            toml::Value::String(s) => Ok(Cmd {
                cmd: s.to_string(),
                images: vec![],
            }),
            toml::Value::Table(table) => {
                if let Some(cmd) = table.get("cmd") {
                    let mut command = Cmd {
                        cmd: String::new(),
                        images: vec![],
                    };
                    if !cmd.is_str() {
                        return Err(anyhow!("expected a string as command, found `{:?}`", cmd));
                    }
                    command.cmd = cmd.as_str().unwrap().to_string();
                    if let Some(images) = table.get("images") {
                        if !images.is_array() {
                            return Err(anyhow!("expected an array of images, found `{:?}`", cmd));
                        }

                        for image in images.as_array().unwrap() {
                            if !image.is_str() {
                                return Err(anyhow!(
                                    "expected a string as image, found `{:?}`",
                                    cmd
                                ));
                            }
                            command.images.push(image.as_str().unwrap().to_string())
                        }
                    }

                    Ok(command)
                } else {
                    Err(anyhow!("expected `cmd` key"))
                }
            }
            val => Err(anyhow!(
                "expected a table or a string as command, found `{:?}`",
                val
            )),
        }
    }
}

impl Cmd {}
