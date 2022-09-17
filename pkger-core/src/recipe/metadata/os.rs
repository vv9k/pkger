use serde::{Deserialize, Serialize};
use std::convert::AsRef;

//####################################################################################################

#[derive(Debug, Deserialize, Clone, Serialize, Eq, PartialEq, Hash)]
pub struct Os {
    distribution: Distro,
    version: Option<String>,
}

impl Os {
    /// If a matching distribution is found returns an Os object, otherwise returns an error.
    pub fn new<O, V>(os: O, version: Option<V>) -> Self
    where
        O: AsRef<str>,
        V: Into<String>,
    {
        Self {
            distribution: Distro::from(os.as_ref()),
            version: version.map(V::into),
        }
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
        let version: u8 = self.version().parse().unwrap_or_default();
        match self.distribution {
            Distro::Arch => PackageManager::Pacman,
            Distro::Debian | Distro::Ubuntu => PackageManager::Apt,
            Distro::Rocky | Distro::RedHat | Distro::CentOS if version >= 8 => PackageManager::Dnf,
            Distro::Fedora if version >= 22 => PackageManager::Dnf,
            Distro::Rocky => PackageManager::Dnf,
            Distro::RedHat | Distro::CentOS | Distro::Fedora => PackageManager::Yum,
            Distro::Alpine => PackageManager::Apk,
            Distro::Unknown => PackageManager::Unknown,
        }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self.distribution, Distro::Unknown)
    }
}

//####################################################################################################

#[allow(clippy::upper_case_acronyms)]
#[derive(Copy, Debug, Deserialize, Clone, Serialize, PartialEq, Eq, Hash)]
pub enum Distro {
    Arch,
    CentOS,
    Debian,
    Fedora,
    RedHat,
    Ubuntu,
    Rocky,
    Alpine,
    Unknown,
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
            Rocky => "rocky",
            Alpine => "alpine",
            Unknown => "unknown",
        }
    }
}

impl From<&str> for Distro {
    fn from(s: &str) -> Self {
        use Distro::*;
        const DISTROS: &[(&str, Distro)] = &[
            ("arch", Arch),
            ("centos", CentOS),
            ("debian", Debian),
            ("fedora", Fedora),
            ("redhat", RedHat),
            ("red hat", RedHat),
            ("ubuntu", Ubuntu),
            ("rocky", Rocky),
            ("alpine", Alpine),
        ];
        let out = s.to_lowercase();
        for (name, distro) in DISTROS.iter() {
            if out.contains(name) {
                return *distro;
            }
        }
        Unknown
    }
}

//####################################################################################################

#[derive(Debug, Clone)]
pub enum PackageManager {
    Apt,
    Dnf,
    Pacman,
    Yum,
    Apk,
    Unknown,
}

impl AsRef<str> for PackageManager {
    fn as_ref(&self) -> &str {
        match self {
            Self::Apt => "apt-get",
            Self::Dnf => "dnf",
            Self::Pacman => "pacman",
            Self::Yum => "yum",
            Self::Apk => "apk",
            Self::Unknown => "unkown",
        }
    }
}

impl PackageManager {
    pub fn install_args(&self) -> Vec<&'static str> {
        match self {
            Self::Apt => vec!["install", "-y"],
            Self::Dnf => vec!["install", "-y"],
            Self::Pacman => vec!["-S", "--noconfirm"],
            Self::Yum => vec!["install", "-y"],
            Self::Apk => vec!["add"],
            Self::Unknown => vec![],
        }
    }

    pub fn update_repos_args(&self) -> Vec<&'static str> {
        match self {
            Self::Apt => vec!["update", "-y"],
            Self::Dnf | Self::Yum => vec!["clean", "metadata"],
            Self::Pacman => vec!["-Sy", "--noconfirm"],
            Self::Apk => vec!["update"],
            Self::Unknown => vec![],
        }
    }

    pub fn upgrade_packages_args(&self) -> Vec<&'static str> {
        match self {
            Self::Apt => vec!["dist-upgrade", "-y"],
            Self::Dnf | Self::Yum => vec!["update", "-y"],
            Self::Pacman => vec!["-Syu", "--noconfirm"],
            Self::Apk => vec!["upgrade"],
            Self::Unknown => vec![],
        }
    }

    pub fn clean_cache(&self) -> Vec<&'static str> {
        match self {
            Self::Apt => vec!["clean"],
            Self::Dnf | Self::Yum => vec!["clean", "metadata"],
            Self::Pacman => vec!["-Sc"],
            Self::Apk => vec!["cache", "clean"],
            Self::Unknown => vec![],
        }
    }

    pub fn should_clean_cache(&self) -> bool {
        #[allow(clippy::match_like_matches_macro)]
        match self {
            Self::Apk => false,
            _ => true,
        }
    }
}
