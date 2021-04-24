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
        }
    }
}

#[derive(Clone, Debug)]
pub struct GitSource {
    url: String,
    // defaults to master
    branch: String,
}

impl From<&str> for GitSource {
    fn from(s: &str) -> Self {
        Self {
            url: s.to_string(),
            branch: "master".to_string(),
        }
    }
}

impl TryFrom<toml::value::Table> for GitSource {
    type Error = Error;
    fn try_from(table: toml::value::Table) -> Result<Self> {
        if let Some(url) = table.get("url") {
            if !url.is_str() {
                return Err(anyhow!("expected a string as url, found `{:?}`", url));
            }

            let url = url.as_str().unwrap().to_string();

            if let Some(branch) = table.get("branch") {
                if !branch.is_str() {
                    return Err(anyhow!("expected a string as branch, found `{:?}`", branch));
                }

                return Ok(GitSource::new(
                    url,
                    Some(branch.as_str().unwrap().to_string()),
                ));
            }

            Ok(GitSource::new(url, None::<&str>))
        } else {
            Err(anyhow!(
                "expected a url entry in a table, found `{:?}`",
                table
            ))
        }
    }
}

impl TryFrom<toml::Value> for GitSource {
    type Error = Error;
    fn try_from(value: toml::Value) -> Result<Self> {
        match value {
            toml::Value::Table(table) => Self::try_from(table),
            toml::Value::String(s) => Ok(Self::from(s.as_str())),
            value => Err(anyhow!(
                "expected a table or a string as git source, found `{:?}`",
                value
            )),
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

impl TryFrom<toml::value::Table> for ImageTarget {
    type Error = Error;

    fn try_from(map: toml::value::Table) -> Result<Self> {
        if let Some(image) = map.get("name") {
            if !image.is_str() {
                return Err(anyhow!(
                    "expected a string as image name, found `{:?}`",
                    image
                ));
            }
            let image = image.as_str().unwrap().to_string();

            let target = if let Some(target) = map.get("target") {
                if !target.is_str() {
                    return Err(anyhow!(
                        "expected a string as image target, found `{:?}`",
                        image
                    ));
                } else {
                    BuildTarget::try_from(target.as_str().unwrap())?
                }
            } else {
                BuildTarget::default()
            };

            Ok(ImageTarget { image, target })
        } else {
            Err(anyhow!("image name not found in `{:?}`", map))
        }
    }
}

impl TryFrom<toml::Value> for ImageTarget {
    type Error = Error;
    fn try_from(value: toml::Value) -> Result<Self> {
        match value {
            toml::Value::Table(map) => Self::try_from(map),
            toml::Value::String(image) => Ok(Self {
                image,
                target: BuildTarget::default(),
            }),
            value => Err(anyhow!(
                "expected a map or string for image, found `{:?}`",
                value
            )),
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
