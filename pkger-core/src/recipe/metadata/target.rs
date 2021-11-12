use crate::{Error, Result};

use serde::{Deserialize, Serialize};
use std::convert::{AsRef, TryFrom};

#[derive(Copy, Clone, Deserialize, Serialize, Debug, Eq, PartialEq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum BuildTarget {
    Rpm,
    Deb,
    Gzip,
    Pkg,
    Apk,
}

impl Default for BuildTarget {
    fn default() -> Self {
        Self::Gzip
    }
}

impl TryFrom<&str> for BuildTarget {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self> {
        match &s.to_lowercase()[..] {
            "rpm" => Ok(Self::Rpm),
            "deb" => Ok(Self::Deb),
            "gzip" => Ok(Self::Gzip),
            "pkg" => Ok(Self::Pkg),
            "apk" => Ok(Self::Apk),
            target => Err(anyhow!("unknown build target `{}`", target)),
        }
    }
}

impl AsRef<str> for BuildTarget {
    fn as_ref(&self) -> &str {
        match &self {
            BuildTarget::Rpm => "rpm",
            BuildTarget::Deb => "deb",
            BuildTarget::Gzip => "gzip",
            BuildTarget::Pkg => "pkg",
            BuildTarget::Apk => "apk",
        }
    }
}
