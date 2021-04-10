use serde::{Deserialize, Serialize};
use std::convert::AsRef;

#[derive(Debug, Clone)]
pub enum PackageManager {
    Apt,
    Dnf,
    Pacman,
    Yum,
    Unknown,
}

impl AsRef<str> for PackageManager {
    fn as_ref(&self) -> &str {
        match self {
            Self::Apt => "apt-get",
            Self::Dnf => "dnf",
            Self::Pacman => "pacman",
            Self::Yum => "yum",
            Self::Unknown => "",
        }
    }
}

impl PackageManager {
    pub fn install_args(&self) -> Vec<&'static str> {
        match self {
            Self::Apt => vec!["install", "-y"],
            Self::Dnf => vec!["install", "-y"],
            Self::Pacman => vec!["-S"],
            Self::Yum => vec!["install", "-y"],
            Self::Unknown => vec![],
        }
    }

    pub fn update_repos_args(&self) -> Vec<&'static str> {
        match self {
            Self::Apt => vec!["update", "-y"],
            Self::Dnf | Self::Yum => vec!["clean", "metadata"],
            Self::Pacman => vec!["-Sy"],
            Self::Unknown => vec![],
        }
    }

    pub fn upgrade_packages_args(&self) -> Vec<&'static str> {
        match self {
            Self::Apt => vec!["dist-upgrade", "-y"],
            Self::Dnf | Self::Yum => vec!["update", "-y"],
            Self::Pacman => vec!["-Syu"],
            Self::Unknown => vec![],
        }
    }
}

// enum holding version of os
#[derive(Debug, Deserialize, Clone, Serialize)]
pub enum Os {
    Arch(String),
    Centos(String),
    Debian(String),
    Fedora(String),
    Redhat(String),
    Ubuntu(String),
    Unknown,
}

impl AsRef<str> for Os {
    fn as_ref(&self) -> &str {
        match self {
            Os::Arch(_) => "arch",
            Os::Centos(_) => "centos",
            Os::Debian(_) => "debian",
            Os::Fedora(_) => "fedora",
            Os::Redhat(_) => "redhat",
            Os::Ubuntu(_) => "ubuntu",
            Os::Unknown => "unknown",
        }
    }
}

impl Os {
    pub fn from(os: Option<String>, version: Option<String>) -> Os {
        if let Some(os) = os {
            let version = version.unwrap_or_default();
            match &os[..] {
                "arch" => Os::Arch(version),
                "centos" => Os::Centos(version),
                "debian" => Os::Debian(version),
                "fedora" => Os::Fedora(version),
                "redhat" => Os::Redhat(version),
                _ => Os::Unknown,
            }
        } else {
            Os::Unknown
        }
    }

    pub fn os_ver(&self) -> &str {
        match self {
            Os::Arch(v) => v.as_str(),
            Os::Centos(v) => v.as_str(),
            Os::Debian(v) => v.as_str(),
            Os::Fedora(v) => v.as_str(),
            Os::Redhat(v) => v.as_str(),
            Os::Ubuntu(v) => v.as_str(),
            Os::Unknown => "",
        }
    }

    #[allow(dead_code)]
    pub fn package_manager(&self) -> PackageManager {
        match self {
            Os::Arch(_) => PackageManager::Pacman,
            Os::Debian(_) | Os::Ubuntu(_) => PackageManager::Apt,
            Os::Redhat(v) | Os::Centos(v) | Os::Fedora(v) if v == "8" => PackageManager::Dnf,
            Os::Redhat(_) | Os::Centos(_) | Os::Fedora(_) => PackageManager::Yum,
            Os::Unknown => PackageManager::Unknown,
        }
    }
}
