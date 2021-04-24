use crate::{Error, Result};

use serde::{Deserialize, Serialize};
use std::convert::{AsRef, TryFrom};

//####################################################################################################

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Os {
    distribution: Distro,
    version: Option<String>,
}

impl Os {
    /// If a matching distribution is found returns an Os object, otherwise returns an error.
    pub fn new<O, V>(os: O, version: Option<V>) -> Result<Self>
    where
        O: AsRef<str>,
        V: Into<String>,
    {
        Ok(Self {
            distribution: Distro::try_from(os.as_ref())?,
            version: version.map(V::into),
        })
    }

    pub fn version(&self) -> &str {
        if let Some(version) = &self.version {
            version.as_str()
        } else {
            ""
        }
    }

    pub fn name(&self) -> &str {
        self.distribution.as_ref()
    }

    pub fn package_manager(&self) -> PackageManager {
        match self.distribution {
            Distro::Arch => PackageManager::Pacman,
            Distro::Debian | Distro::Ubuntu => PackageManager::Apt,
            Distro::RedHat | Distro::CentOS | Distro::Fedora
                if self.version == Some("8".to_string()) =>
            {
                PackageManager::Dnf
            }
            Distro::RedHat | Distro::CentOS | Distro::Fedora => PackageManager::Yum,
        }
    }
}

//####################################################################################################

#[derive(Debug, Deserialize, Clone, Serialize, PartialEq, Eq)]
pub enum Distro {
    Arch,
    CentOS,
    Debian,
    Fedora,
    RedHat,
    Ubuntu,
}

impl AsRef<str> for Distro {
    fn as_ref(&self) -> &str {
        use Distro::*;
        match self {
            Arch => "arch",
            CentOS => "centos",
            Debian => "debian",
            Fedora => "fedora",
            RedHat => "redhat",
            Ubuntu => "ubuntu",
        }
    }
}

impl TryFrom<&str> for Distro {
    type Error = Error;
    fn try_from(s: &str) -> Result<Self> {
        use Distro::*;
        match &s.to_lowercase()[..] {
            "arch" => Ok(Arch),
            "centos" => Ok(CentOS),
            "debian" => Ok(Debian),
            "fedora" => Ok(Fedora),
            "redhat" => Ok(RedHat),
            os => Err(anyhow!("unknown distribution `{}`", os)),
        }
    }
}

//####################################################################################################

#[derive(Debug, Clone)]
pub enum PackageManager {
    Apt,
    Dnf,
    Pacman,
    Yum,
}

impl AsRef<str> for PackageManager {
    fn as_ref(&self) -> &str {
        match self {
            Self::Apt => "apt-get",
            Self::Dnf => "dnf",
            Self::Pacman => "pacman",
            Self::Yum => "yum",
        }
    }
}

#[allow(dead_code)]
impl PackageManager {
    pub fn install_args(&self) -> Vec<&'static str> {
        match self {
            Self::Apt => vec!["install", "-y"],
            Self::Dnf => vec!["install", "-y"],
            Self::Pacman => vec!["-S"],
            Self::Yum => vec!["install", "-y"],
        }
    }

    pub fn update_repos_args(&self) -> Vec<&'static str> {
        match self {
            Self::Apt => vec!["update", "-y"],
            Self::Dnf | Self::Yum => vec!["clean", "metadata"],
            Self::Pacman => vec!["-Sy"],
        }
    }

    pub fn upgrade_packages_args(&self) -> Vec<&'static str> {
        match self {
            Self::Apt => vec!["dist-upgrade", "-y"],
            Self::Dnf | Self::Yum => vec!["update", "-y"],
            Self::Pacman => vec!["-Syu"],
        }
    }
}
