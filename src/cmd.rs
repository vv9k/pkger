use crate::{Error, Result};

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

impl From<&str> for Cmd {
    fn from(s: &str) -> Self {
        Self {
            cmd: s.to_string(),
            images: vec![],
        }
    }
}

impl TryFrom<toml::value::Table> for Cmd {
    type Error = Error;
    fn try_from(table: toml::value::Table) -> Result<Self> {
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
                        return Err(anyhow!("expected a string as image, found `{:?}`", cmd));
                    }
                    command.images.push(image.as_str().unwrap().to_string())
                }
            }

            Ok(command)
        } else {
            Err(anyhow!("expected `cmd` key"))
        }
    }
}

impl TryFrom<toml::Value> for Cmd {
    type Error = crate::Error;

    fn try_from(value: toml::Value) -> Result<Self> {
        match value {
            toml::Value::String(s) => Ok(Cmd::from(s.as_str())),
            toml::Value::Table(table) => Self::try_from(table),
            val => Err(anyhow!(
                "expected a table or a string as command, found `{:?}`",
                val
            )),
        }
    }
}

impl Cmd {}
