use crate::deps::Dependencies;
use crate::{Error, Result};

use serde::{Deserialize, Serialize};
use std::convert::{AsRef, TryFrom};

#[derive(Clone, Deserialize, Serialize, Debug, Eq, PartialEq, Hash)]
pub enum BuildTarget {
    Rpm,
    Deb,
    Gzip,
}

impl From<Option<String>> for BuildTarget {
    fn from(s: Option<String>) -> Self {
        match s.map(|inner| inner.to_lowercase()) {
            Some(s) if &s == "rpm" => Self::Rpm,
            Some(s) if &s == "deb" => Self::Deb,
            _ => Self::Gzip,
        }
    }
}

impl AsRef<str> for BuildTarget {
    fn as_ref(&self) -> &str {
        match &self {
            BuildTarget::Rpm => "rpm",
            BuildTarget::Deb => "deb",
            BuildTarget::Gzip => "gzip",
        }
    }
}

#[derive(Clone, Debug)]
pub struct GitSource {
    url: String,
    // defaults to master
    branch: String,
}
impl TryFrom<toml::Value> for GitSource {
    type Error = Error;
    fn try_from(value: toml::Value) -> Result<Self> {
        if let Some(url) = value.get("url") {
            Ok(GitSource::new(
                url.to_string(),
                value.get("branch").map(toml::Value::to_string),
            ))
        } else if value.is_str() {
            Ok(GitSource::new(value.to_string(), None::<&str>))
        } else {
            Err(anyhow!(
                "git source entry missing url `{}`",
                value.to_string()
            ))
        }
    }
}
impl GitSource {
    pub fn new<U, B>(url: U, branch: Option<B>) -> Self
    where
        U: Into<String>,
        B: Into<String>,
    {
        Self {
            url: url.into(),
            branch: branch.map(B::into).unwrap_or_else(|| "master".to_string()),
        }
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn branch(&self) -> &str {
        &self.branch
    }
}

#[derive(Clone, Deserialize, Serialize, Debug, Eq, PartialEq, Hash)]
pub struct ImageTarget {
    pub image: String,
    pub target: BuildTarget,
}

impl ImageTarget {
    pub fn new<I: Into<String>>(image: I, target: &BuildTarget) -> Self {
        Self {
            image: image.into(),
            target: target.clone(),
        }
    }
}

impl TryFrom<toml::Value> for ImageTarget {
    type Error = Error;
    fn try_from(value: toml::Value) -> Result<Self> {
        if let Some(image) = value.get("name") {
            Ok(Self {
                image: image.to_string().trim_matches('"').to_string(),
                target: BuildTarget::from(
                    value
                        .get("target")
                        .map(|v| v.to_string().trim_matches('"').to_string()),
                ),
            })
        } else {
            Err(anyhow!("image entry missing name `{}`", value.to_string()))
        }
    }
}

#[derive(Clone, Debug)]
pub struct Metadata {
    // General
    pub name: String,
    pub version: String,
    pub description: String,
    pub license: String,
    pub images: Vec<ImageTarget>,

    pub maintainer: Option<String>,
    pub arch: Option<String>,
    /// http/https or file system source pointing to a tar.gz or tar.xz package
    pub source: Option<String>,
    /// Git repository as source
    pub git: Option<GitSource>,
    /// Whether default dependencies should be installed before the build
    pub skip_default_deps: Option<bool>,
    /// Directories to exclude when creating the package
    pub exclude: Option<Vec<String>>,

    pub build_depends: Option<Dependencies>,

    pub depends: Option<Dependencies>,
    pub conflicts: Option<Dependencies>,
    pub provides: Option<Dependencies>,

    // Only DEB
    pub section: Option<String>,
    pub priority: Option<String>,

    // Only RPM
    pub release: Option<String>,
    pub obsoletes: Option<Dependencies>,
    pub summary: Option<String>,
}

impl Metadata {
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
}

impl TryFrom<MetadataRep> for Metadata {
    type Error = Error;

    fn try_from(rep: MetadataRep) -> Result<Self> {
        let build_depends = if let Some(deps) = rep.build_depends {
            Some(Dependencies::try_from(deps)?)
        } else {
            None
        };
        let depends = if let Some(deps) = rep.depends {
            Some(Dependencies::try_from(deps)?)
        } else {
            None
        };
        let obsoletes = if let Some(deps) = rep.obsoletes {
            Some(Dependencies::try_from(deps)?)
        } else {
            None
        };
        let conflicts = if let Some(deps) = rep.conflicts {
            Some(Dependencies::try_from(deps)?)
        } else {
            None
        };
        let provides = if let Some(deps) = rep.provides {
            Some(Dependencies::try_from(deps)?)
        } else {
            None
        };

        let mut images = vec![];
        for image in rep.images.into_iter().map(ImageTarget::try_from) {
            images.push(image?);
        }

        Ok(Self {
            name: rep.name,
            version: rep.version,
            arch: rep.arch,
            release: rep.release,
            description: rep.description,
            license: rep.license,
            source: rep.source,
            images,
            git: {
                if let Some(val) = rep.git {
                    GitSource::try_from(val).map(Some)?
                } else {
                    None
                }
            },
            depends,
            skip_default_deps: rep.skip_default_deps,
            build_depends,
            obsoletes,
            conflicts,
            provides,
            exclude: rep.exclude,
            maintainer: rep.maintainer,
            section: rep.section,
            priority: rep.priority,
            summary: rep.summary,
        })
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct MetadataRep {
    // Required
    pub name: String,
    pub version: String,
    pub description: String,
    pub license: String,
    pub images: Vec<toml::Value>,

    // Common optional
    pub maintainer: Option<String>,
    pub arch: Option<String>,
    /// http/https or file system source pointing to a tar.gz or tar.xz package
    pub source: Option<String>,
    /// Git repository as source
    pub git: Option<toml::Value>,
    /// Whether to install default dependencies before build
    pub skip_default_deps: Option<bool>,
    /// Directories to exclude when creating the package
    pub exclude: Option<Vec<String>>,

    pub build_depends: Option<toml::Value>,
    pub depends: Option<toml::Value>,
    pub conflicts: Option<toml::Value>,
    pub provides: Option<toml::Value>,

    // Only DEB
    pub section: Option<String>,
    pub priority: Option<String>,

    // Only RPM
    pub release: Option<String>,
    pub obsoletes: Option<toml::Value>,
    pub summary: Option<String>,
}
