mod arch;
mod deps;
mod git;
mod image;
mod os;
mod target;

pub use arch::BuildArch;
pub use deps::Dependencies;
pub use git::GitSource;
pub use image::ImageTarget;
pub use os::{Distro, Os, PackageManager};
pub use target::BuildTarget;

use crate::{Error, Result};

use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
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

    pub images: Option<Vec<YamlValue>>,

    // Common optional
    pub maintainer: Option<String>,
    /// The URL of the web site for this package
    pub url: Option<String>,
    pub arch: Option<String>,
    /// http/https or file system source pointing to a tar.gz or tar.xz package
    pub source: Option<String>,
    /// Git repository as source
    pub git: Option<YamlValue>,
    /// Whether to install default dependencies before build
    pub skip_default_deps: Option<bool>,
    /// Directories to exclude when creating the package
    pub exclude: Option<Vec<String>>,
    pub group: Option<String>,
    /// The release number. This is usually a positive integer number that allows to differentiate
    /// between consecutive builds of the same version of a package
    pub release: Option<String>,
    /// Used to force the package to be seen as newer than any previous version with a lower epoch
    pub epoch: Option<String>,

    pub build_depends: Option<YamlValue>,
    pub depends: Option<YamlValue>,
    pub conflicts: Option<YamlValue>,
    pub provides: Option<YamlValue>,

    // Only DEB
    pub deb: Option<DebRep>,

    // Only RPM
    pub rpm: Option<RpmRep>,

    // Only PKG
    pub pkg: Option<PkgRep>,
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct PkgRep {
    /// The name of the .install script to be included in the package
    pub install: Option<String>,
    /// A list of files that can contain user-made changes and should be preserved during upgrade
    /// or removal of a package
    pub backup: Option<Vec<String>>,
    pub replaces: Option<YamlValue>,
    /// Optional dependencies needed for full functionality of the package
    pub optdepends: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct PkgInfo {
    /// The name of the .install script to be included in the package
    pub install: Option<String>,
    /// A list of files that can contain user-made changes and should be preserved during upgrade
    /// or removal of a package
    pub backup: Option<Vec<String>>,
    pub replaces: Option<Dependencies>,
    /// Optional dependencies needed for full functionality of the package
    pub optdepends: Option<Vec<String>>,
}

impl TryFrom<PkgRep> for PkgInfo {
    type Error = Error;

    fn try_from(rep: PkgRep) -> Result<Self> {
        Ok(Self {
            install: rep.install,
            backup: rep.backup,
            replaces: let_some_deps!(rep.replaces),
            optdepends: rep.optdepends,
        })
    }
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct DebRep {
    pub priority: Option<String>,
    pub installed_size: Option<String>,
    pub built_using: Option<String>,
    pub essential: Option<bool>,

    pub pre_depends: Option<YamlValue>,
    pub recommends: Option<YamlValue>,
    pub suggests: Option<YamlValue>,
    pub breaks: Option<YamlValue>,
    pub replaces: Option<YamlValue>,
    pub enchances: Option<YamlValue>,
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
    pub obsoletes: Option<YamlValue>,
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
    pub arch: BuildArch,

    pub images: Option<Vec<ImageTarget>>,
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
    /// The release number. This is usually a positive integer number that allows to differentiate
    /// between consecutive builds of the same version of a package
    pub release: Option<String>,
    /// Used to force the package to be seen as newer than any previous version with a lower epoch
    pub epoch: Option<String>,

    pub build_depends: Option<Dependencies>,

    pub depends: Option<Dependencies>,
    pub conflicts: Option<Dependencies>,
    pub provides: Option<Dependencies>,

    pub deb: Option<DebInfo>,

    pub rpm: Option<RpmInfo>,

    pub pkg: Option<PkgInfo>,
}

impl Metadata {
    /// Returns the release number of this package if one exists, otherwise returns "0"
    pub fn release(&self) -> &str {
        if let Some(release) = &self.release {
            release.as_str()
        } else {
            "0"
        }
    }
}

impl TryFrom<MetadataRep> for Metadata {
    type Error = Error;

    fn try_from(rep: MetadataRep) -> Result<Self> {
        let images = if let Some(rep_images) = rep.images {
            let mut images = vec![];
            for image in rep_images.into_iter().map(ImageTarget::try_from) {
                images.push(image?);
            }
            Some(images)
        } else {
            None
        };

        Ok(Self {
            name: rep.name,
            version: rep.version,
            description: rep.description,
            license: rep.license,
            images,

            arch: rep
                .arch
                .map(|arch| BuildArch::from(arch.as_str()))
                .unwrap_or_else(|| BuildArch::All),
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
            release: rep.release,
            epoch: rep.epoch,

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
