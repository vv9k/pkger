use serde::{Deserialize, Serialize};

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
    pub fn package_manager(&self) -> &str {
        match self {
            Os::Arch(_) => "pacman",
            Os::Debian(_) | Os::Ubuntu(_) => "apt-get",
            Os::Redhat(v) | Os::Centos(v) | Os::Fedora(v) if v == "8" => "dnf",
            Os::Redhat(_) | Os::Centos(_) | Os::Fedora(_) => "yum",
            Os::Unknown => "",
        }
    }
}
