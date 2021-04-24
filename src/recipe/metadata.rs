mod git;
mod image;
mod target;

pub use git::GitSource;
pub use image::ImageTarget;
pub use target::BuildTarget;

use crate::deps::Dependencies;
use crate::{Error, Result};

use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

macro_rules! let_some_deps {
    ($from:expr) => {
        if let Some(deps) = $from {
            Some(Dependencies::try_from(deps)?)
        } else {
            None
        }
    };
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct MetadataRep {
    // Required
    pub name: String,
    pub version: String,
    pub description: String,
    pub license: String,
    pub images: Vec<toml::Value>,

    // Common optional
    pub maintainer: Option<String>,
    /// The URL of the web site for this package
    pub url: Option<String>,
    pub arch: Option<String>,
    /// http/https or file system source pointing to a tar.gz or tar.xz package
    pub source: Option<String>,
    /// Git repository as source
    pub git: Option<toml::Value>,
    /// Whether to install default dependencies before build
    pub skip_default_deps: Option<bool>,
    /// Directories to exclude when creating the package
    pub exclude: Option<Vec<String>>,
    pub group: Option<String>,

    pub build_depends: Option<toml::Value>,
    pub depends: Option<toml::Value>,
    pub conflicts: Option<toml::Value>,
    pub provides: Option<toml::Value>,

    // Only DEB
    pub deb: Option<DebRep>,

    // Only RPM
    pub rpm: Option<RpmRep>,

    // Only PKG
    pub pkg: Option<PkgRep>,
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct PkgRep {
    pub pkgrel: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PkgInfo {
    pub pkgrel: Option<String>,
}

impl TryFrom<PkgRep> for PkgInfo {
    type Error = Error;

    fn try_from(rep: PkgRep) -> Result<Self> {
        Ok(Self { pkgrel: rep.pkgrel })
    }
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct DebRep {
    pub priority: Option<String>,
    pub installed_size: Option<String>,
    pub built_using: Option<String>,
    pub essential: Option<bool>,

    pub pre_depends: Option<toml::Value>,
    pub recommends: Option<toml::Value>,
    pub suggests: Option<toml::Value>,
    pub breaks: Option<toml::Value>,
    pub replaces: Option<toml::Value>,
    pub enchances: Option<toml::Value>,
}

#[derive(Clone, Debug)]
pub struct DebInfo {
    pub priority: Option<String>,
    pub installed_size: Option<String>,
    pub built_using: Option<String>,
    pub essential: Option<bool>,

    pub pre_depends: Option<Dependencies>,
    pub recommends: Option<Dependencies>,
    pub suggests: Option<Dependencies>,
    pub breaks: Option<Dependencies>,
    pub replaces: Option<Dependencies>,
    pub enchances: Option<Dependencies>,
}

impl TryFrom<DebRep> for DebInfo {
    type Error = Error;

    fn try_from(rep: DebRep) -> Result<Self> {
        Ok(Self {
            priority: rep.priority,
            installed_size: rep.installed_size,
            built_using: rep.built_using,
            essential: rep.essential,

            pre_depends: let_some_deps!(rep.pre_depends),
            recommends: let_some_deps!(rep.recommends),
            suggests: let_some_deps!(rep.suggests),
            breaks: let_some_deps!(rep.breaks),
            replaces: let_some_deps!(rep.replaces),
            enchances: let_some_deps!(rep.enchances),
        })
    }
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct RpmRep {
    pub obsoletes: Option<toml::Value>,
    pub release: Option<String>,
    pub epoch: Option<String>,
    pub vendor: Option<String>,
    pub icon: Option<String>,
    pub summary: Option<String>,
    pub pre_script: Option<String>,
    pub post_script: Option<String>,
    pub preun_script: Option<String>,
    pub postun_script: Option<String>,
    pub config_noreplace: Option<String>,
}

impl TryFrom<RpmRep> for RpmInfo {
    type Error = Error;

    fn try_from(rep: RpmRep) -> Result<Self> {
        Ok(Self {
            obsoletes: let_some_deps!(rep.obsoletes),
            release: rep.release,
            epoch: rep.epoch,
            vendor: rep.vendor,
            icon: rep.icon,
            summary: rep.summary,
            pre_script: rep.pre_script,
            post_script: rep.post_script,
            preun_script: rep.preun_script,
            postun_script: rep.postun_script,
            config_noreplace: rep.config_noreplace,
        })
    }
}

#[derive(Clone, Debug)]
pub struct RpmInfo {
    pub obsoletes: Option<Dependencies>,
    pub release: Option<String>,
    pub epoch: Option<String>,
    pub vendor: Option<String>,
    pub icon: Option<String>,
    pub summary: Option<String>,
    pub pre_script: Option<String>,
    pub post_script: Option<String>,
    pub preun_script: Option<String>,
    pub postun_script: Option<String>,
    pub config_noreplace: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Metadata {
    // General
    pub name: String,
    pub version: String,
    pub description: String,
    pub license: String,
    pub images: Vec<ImageTarget>,

    pub arch: Option<String>,
    pub maintainer: Option<String>,
    /// The URL of the web site for this package
    pub url: Option<String>,
    /// http/https or file system source pointing to a tar.gz or tar.xz package
    pub source: Option<String>,
    /// Git repository as source
    pub git: Option<GitSource>,
    /// Whether default dependencies should be installed before the build
    pub skip_default_deps: Option<bool>,
    /// Directories to exclude when creating the package
    pub exclude: Option<Vec<String>>,
    /// Works as section in DEB and group in RPM
    pub group: Option<String>,

    pub build_depends: Option<Dependencies>,

    pub depends: Option<Dependencies>,
    pub conflicts: Option<Dependencies>,
    pub provides: Option<Dependencies>,

    pub deb: Option<DebInfo>,

    pub rpm: Option<RpmInfo>,

    pub pkg: Option<PkgInfo>,
}

impl Metadata {
    /// Returns the name of `arch` appropriate for DEB build
    pub fn deb_arch(&self) -> &str {
        if let Some(arch) = &self.arch {
            match &arch[..] {
                "amd64" | "x86_64" => "amd64",
                "x86" | "i386" => "i386",
                arch => arch,
                // #TODO: add more...
            }
        } else {
            "all"
        }
    }

    /// Returns the name of `arch` appropriate for RPM build
    pub fn rpm_arch(&self) -> &str {
        if let Some(arch) = &self.arch {
            match &arch[..] {
                "amd64" | "x86_64" => "x86_64",
                "x86" | "i386" => "x86",
                arch => arch,
                // #TODO: add more...
            }
        } else {
            "noarch"
        }
    }

    /// Returns the name of `arch` appropriate for PKG build
    pub fn pkg_arch(&self) -> &str {
        if let Some(arch) = &self.arch {
            match &arch[..] {
                "amd64" | "x86_64" => "x86_64",
                "x86" | "i386" => "x86",
                arch => arch,
                // #TODO: add more...
            }
        } else {
            "any"
        }
    }

    /// Returns the RPM release if the value is available, otherwise returns "0"
    pub fn rpm_release(&self) -> &str {
        if let Some(rpm) = &self.rpm {
            if let Some(release) = &rpm.release {
                return release.as_str();
            }
        }

        "0"
    }
}

impl TryFrom<MetadataRep> for Metadata {
    type Error = Error;

    fn try_from(rep: MetadataRep) -> Result<Self> {
        let mut images = vec![];
        for image in rep.images.into_iter().map(ImageTarget::try_from) {
            images.push(image?);
        }

        Ok(Self {
            name: rep.name,
            version: rep.version,
            description: rep.description,
            license: rep.license,
            images,

            arch: rep.arch,
            maintainer: rep.maintainer,
            url: rep.url,
            source: rep.source,
            git: {
                if let Some(val) = rep.git {
                    GitSource::try_from(val).map(Some)?
                } else {
                    None
                }
            },
            skip_default_deps: rep.skip_default_deps,
            exclude: rep.exclude,
            group: rep.group,

            build_depends: let_some_deps!(rep.build_depends),

            depends: let_some_deps!(rep.depends),
            conflicts: let_some_deps!(rep.conflicts),
            provides: let_some_deps!(rep.provides),

            deb: if let Some(deb) = rep.deb {
                Some(DebInfo::try_from(deb)?)
            } else {
                None
            },

            rpm: if let Some(rpm) = rep.rpm {
                Some(RpmInfo::try_from(rpm)?)
            } else {
                None
            },

            pkg: if let Some(pkg) = rep.pkg {
                Some(PkgInfo::try_from(pkg)?)
            } else {
                None
            },
        })
    }
}
