mod arch;
mod deps;
mod git;
mod image;
mod os;
mod patches;
mod target;

pub use arch::BuildArch;
pub use deps::Dependencies;
pub use git::GitSource;
pub use image::{deserialize_images, ImageTarget};
pub use os::{Distro, Os, PackageManager};
pub use patches::{Patch, Patches};
pub use target::BuildTarget;

use crate::{Error, Result};

use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use std::convert::TryFrom;

macro_rules! if_let_some_ty {
    ($from:expr, $ty:tt) => {
        if let Some(it) = $from {
            $ty::try_from(it).map(Some)?
        } else {
            None
        }
    };
}

fn null() -> YamlValue {
    YamlValue::Null
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct MetadataRep {
    // Required
    pub name: String,
    pub version: String,
    pub description: String,
    pub license: String,

    #[serde(default)]
    /// If specified all images will apply to this metadata and `images` will be ignored.
    pub all_images: bool,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    // Common optional
    pub maintainer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// The URL of the web site for this package
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// http/https or file system source pointing to a tar.gz or tar.xz package
    pub source: Option<String>,
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    /// Git repository as source
    pub git: YamlValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Whether to install default dependencies before build
    pub skip_default_deps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Directories to exclude when creating the package
    pub exclude: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// The release number. This is usually a positive integer number that allows to differentiate
    /// between consecutive builds of the same version of a package
    pub release: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Used to force the package to be seen as newer than any previous version with a lower epoch
    pub epoch: Option<String>,

    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub build_depends: YamlValue,
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub depends: YamlValue,
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub conflicts: YamlValue,
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub provides: YamlValue,

    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    /// Patches to be applied to the source code. Can be specified only for certain images same
    /// as dependencies.
    pub patches: YamlValue,

    #[serde(skip_serializing_if = "Option::is_none")]
    // Only DEB
    pub deb: Option<DebRep>,

    #[serde(skip_serializing_if = "Option::is_none")]
    // Only RPM
    pub rpm: Option<RpmRep>,

    #[serde(skip_serializing_if = "Option::is_none")]
    // Only PKG
    pub pkg: Option<PkgRep>,

    #[serde(skip_serializing_if = "Option::is_none")]
    // Only APK
    pub apk: Option<ApkRep>,
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct PkgRep {
    #[serde(skip_serializing_if = "Option::is_none")]
    /// The name of the .install script to be included in the package
    pub install: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub backup: Vec<String>,
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub replaces: YamlValue,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub optdepends: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PkgInfo {
    /// The name of the .install script to be included in the package
    pub install: Option<String>,
    /// A list of files that can contain user-made changes and should be preserved during upgrade
    /// or removal of a package
    pub backup: Vec<String>,
    pub replaces: Option<Dependencies>,
    /// Optional dependencies needed for full functionality of the package
    pub optdepends: Vec<String>,
}

impl TryFrom<PkgRep> for PkgInfo {
    type Error = Error;

    fn try_from(rep: PkgRep) -> Result<Self> {
        Ok(Self {
            install: rep.install,
            backup: rep.backup,
            replaces: Dependencies::try_from(rep.replaces).ok(),
            optdepends: rep.optdepends,
        })
    }
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct DebRep {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub built_using: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub essential: Option<bool>,

    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub pre_depends: YamlValue,
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub recommends: YamlValue,
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub suggests: YamlValue,
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub breaks: YamlValue,
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub replaces: YamlValue,
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub enhances: YamlValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebInfo {
    pub priority: Option<String>,
    pub built_using: Option<String>,
    pub essential: Option<bool>,

    pub pre_depends: Option<Dependencies>,
    pub recommends: Option<Dependencies>,
    pub suggests: Option<Dependencies>,
    pub breaks: Option<Dependencies>,
    pub replaces: Option<Dependencies>,
    pub enhances: Option<Dependencies>,
}

impl TryFrom<DebRep> for DebInfo {
    type Error = Error;

    fn try_from(rep: DebRep) -> Result<Self> {
        Ok(Self {
            priority: rep.priority,
            built_using: rep.built_using,
            essential: rep.essential,

            pre_depends: Dependencies::try_from(rep.pre_depends).ok(),
            recommends: Dependencies::try_from(rep.recommends).ok(),
            suggests: Dependencies::try_from(rep.suggests).ok(),
            breaks: Dependencies::try_from(rep.breaks).ok(),
            replaces: Dependencies::try_from(rep.replaces).ok(),
            enhances: Dependencies::try_from(rep.enhances).ok(),
        })
    }
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct RpmRep {
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub obsoletes: YamlValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_req_prov: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preun_script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postun_script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_noreplace: Option<String>,
}

impl TryFrom<RpmRep> for RpmInfo {
    type Error = Error;

    fn try_from(rep: RpmRep) -> Result<Self> {
        Ok(Self {
            obsoletes: Dependencies::try_from(rep.obsoletes).ok(),
            vendor: rep.vendor,
            icon: rep.icon,
            summary: rep.summary,
            auto_req_prov: rep.auto_req_prov.unwrap_or(true),
            pre_script: rep.pre_script,
            post_script: rep.post_script,
            preun_script: rep.preun_script,
            postun_script: rep.postun_script,
            config_noreplace: rep.config_noreplace,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RpmInfo {
    pub obsoletes: Option<Dependencies>,
    pub vendor: Option<String>,
    pub icon: Option<String>,
    pub summary: Option<String>,
    pub auto_req_prov: bool,
    pub pre_script: Option<String>,
    pub post_script: Option<String>,
    pub preun_script: Option<String>,
    pub postun_script: Option<String>,
    pub config_noreplace: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Metadata {
    // General
    pub name: String,
    pub version: String,
    pub description: String,
    pub license: String,
    pub arch: BuildArch,

    pub all_images: bool,
    pub images: Vec<String>,
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

    pub patches: Option<Patches>,

    pub deb: Option<DebInfo>,

    pub rpm: Option<RpmInfo>,

    pub pkg: Option<PkgInfo>,

    pub apk: Option<ApkInfo>,
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
        Ok(Self {
            name: rep.name,
            version: rep.version,
            description: rep.description,
            license: rep.license,
            all_images: rep.all_images,
            images: rep.images,

            arch: rep
                .arch
                .map(|arch| BuildArch::from(arch.as_str()))
                .unwrap_or_else(|| BuildArch::All),
            maintainer: rep.maintainer,
            url: rep.url,
            source: rep.source,
            git: GitSource::try_from(rep.git).ok(),
            skip_default_deps: rep.skip_default_deps,
            exclude: rep.exclude,
            group: rep.group,
            release: rep.release,
            epoch: rep.epoch,

            build_depends: Dependencies::try_from(rep.build_depends).ok(),
            depends: Dependencies::try_from(rep.depends).ok(),
            conflicts: Dependencies::try_from(rep.conflicts).ok(),
            provides: Dependencies::try_from(rep.provides).ok(),

            patches: Patches::try_from(rep.patches).ok(),

            deb: if_let_some_ty!(rep.deb, DebInfo),
            rpm: if_let_some_ty!(rep.rpm, RpmInfo),
            pkg: if_let_some_ty!(rep.pkg, PkgInfo),
            apk: if_let_some_ty!(rep.apk, ApkInfo),
        })
    }
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct ApkRep {
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    /// List of install scripts like pre-install and post-install
    pub install: Vec<String>,
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub replaces: YamlValue,
    #[serde(default = "null")]
    #[serde(skip_serializing_if = "YamlValue::is_null")]
    pub checkdepends: YamlValue,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<std::path::PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApkInfo {
    pub install: Vec<String>,
    pub replaces: Option<Dependencies>,
    pub checkdepends: Option<Dependencies>,
    pub private_key: Option<std::path::PathBuf>,
}

impl TryFrom<ApkRep> for ApkInfo {
    type Error = Error;

    fn try_from(rep: ApkRep) -> Result<Self> {
        Ok(Self {
            install: rep.install,
            replaces: Dependencies::try_from(rep.replaces).ok(),
            checkdepends: Dependencies::try_from(rep.checkdepends).ok(),
            private_key: rep.private_key,
        })
    }
}
