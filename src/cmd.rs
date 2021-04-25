use crate::recipe::BuildTarget;
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
///
/// { cmd = "echo 321", rpm = true } # execute only when building rpm target
pub struct Cmd {
    pub cmd: String,
    pub images: Vec<String>,
    pub rpm: Option<bool>,
    pub deb: Option<bool>,
    pub pkg: Option<bool>,
    pub gzip: Option<bool>,
}

impl From<&str> for Cmd {
    fn from(s: &str) -> Self {
        Self {
            cmd: s.to_string(),
            images: vec![],
            rpm: None,
            deb: None,
            pkg: None,
            gzip: None,
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
                rpm: None,
                deb: None,
                pkg: None,
                gzip: None,
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

            if let Some(rpm) = table.get("rpm") {
                if !rpm.is_bool() {
                    return Err(anyhow!("expected a boolean for rpm, found `{:?}`, rpm"));
                }
                command.rpm = Some(rpm.as_bool().unwrap());
            }
            if let Some(deb) = table.get("deb") {
                if !deb.is_bool() {
                    return Err(anyhow!("expected a boolean for deb, found `{:?}`, deb"));
                }
                command.deb = Some(deb.as_bool().unwrap());
            }
            if let Some(pkg) = table.get("pkg") {
                if !pkg.is_bool() {
                    return Err(anyhow!("expected a boolean for pkg, found `{:?}`, pkg"));
                }
                command.pkg = Some(pkg.as_bool().unwrap());
            }
            if let Some(gzip) = table.get("gzip") {
                if !gzip.is_bool() {
                    return Err(anyhow!("expected a boolean for gzip, found `{:?}`, gzip"));
                }
                command.gzip = Some(gzip.as_bool().unwrap());
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

impl Cmd {
    pub fn should_run_on(&self, target: &BuildTarget) -> bool {
        if self.rpm.is_none() && self.deb.is_none() && self.pkg.is_none() && self.gzip.is_none() {
            return true;
        }
        match &target {
            BuildTarget::Rpm => self.rpm.unwrap_or_default(),
            BuildTarget::Deb => self.deb.unwrap_or_default(),
            BuildTarget::Pkg => self.pkg.unwrap_or_default(),
            BuildTarget::Gzip => self.gzip.unwrap_or_default(),
        }
    }
}
