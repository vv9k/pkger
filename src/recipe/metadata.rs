use crate::deps::Dependencies;
use crate::{Error, Result};

use serde::Deserialize;
use std::convert::TryFrom;

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
pub struct ImageTarget {
    pub image: String,
    pub target: BuildTarget,
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
    pub arch: String,
    pub revision: String,
    pub description: String,
    pub license: String,
    pub source: String,
    pub images: Vec<ImageTarget>,

    // Git repository as source
    pub git: Option<String>,

    // Whether default dependencies should be installed before the build
    pub skip_default_deps: Option<bool>,

    pub build_depends: Option<Dependencies>,
    pub depends: Option<Dependencies>,
    pub obsoletes: Option<Dependencies>,
    pub conflicts: Option<Dependencies>,
    pub provides: Option<Dependencies>,

    // Directories to exclude when creating the package
    pub exclude: Option<Vec<String>>,

    // Only Debian based
    pub maintainer: Option<String>,
    pub section: Option<String>,
    pub priority: Option<String>,
}

impl TryFrom<MetadataRep> for Metadata {
    type Error = Error;

    fn try_from(rep: MetadataRep) -> Result<Self> {
        let build_depends = if let Some(deps) = rep.build_depends {
            Some(Dependencies::new(deps)?)
        } else {
            None
        };
        let depends = if let Some(deps) = rep.depends {
            Some(Dependencies::new(deps)?)
        } else {
            None
        };
        let obsoletes = if let Some(deps) = rep.obsoletes {
            Some(Dependencies::new(deps)?)
        } else {
            None
        };
        let conflicts = if let Some(deps) = rep.conflicts {
            Some(Dependencies::new(deps)?)
        } else {
            None
        };
        let provides = if let Some(deps) = rep.provides {
            Some(Dependencies::new(deps)?)
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
            revision: rep.revision,
            description: rep.description,
            license: rep.license,
            source: rep.source,
            images,
            git: rep.git,
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
        })
    }
}

#[derive(Deserialize, Debug)]
pub struct MetadataRep {
    // General
    pub name: String,
    pub version: String,
    pub arch: String,
    pub revision: String,
    pub description: String,
    pub license: String,
    pub source: String,
    pub images: Vec<toml::Value>,

    /// Git repository as source
    pub git: Option<String>,

    /// Whether to install default dependencies before build
    pub skip_default_deps: Option<bool>,
    pub build_depends: Option<Vec<String>>,
    pub depends: Option<Vec<String>>,
    pub obsoletes: Option<Vec<String>>,
    pub conflicts: Option<Vec<String>>,
    pub provides: Option<Vec<String>>,

    /// Directories to exclude when creating the package
    pub exclude: Option<Vec<String>>,

    // Only Debian based
    pub maintainer: Option<String>,
    pub section: Option<String>,
    pub priority: Option<String>,
}
