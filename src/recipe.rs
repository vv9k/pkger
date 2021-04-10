use crate::cmd::Cmd;
use crate::deps::Dependencies;
use crate::{Error, Result};

use log::error;
use serde::Deserialize;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::env;
use std::fs::{self, DirEntry};
use std::path::Path;

const DEFAULT_RECIPE_FILE: &str = "recipe.toml";

#[derive(Debug, Default)]
pub struct Recipes(HashMap<String, Recipe>);

impl Recipes {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut recipes = Recipes::default();
        let path = env::current_dir()?.join(path.as_ref());

        if !path.is_dir() {
            return Ok(recipes);
        }

        for entry in fs::read_dir(path)? {
            match entry {
                Ok(entry) => {
                    let filename = entry.file_name().to_string_lossy().to_string();
                    match RecipeRep::try_from(entry) {
                        Ok(recipe) => {
                            recipes.0.insert(filename, Recipe::try_from(recipe)?);
                        }
                        Err(e) => error!("failed to read recipe - {}", e),
                    }
                }
                Err(e) => error!("invalid entry - {}", e),
            }
        }

        Ok(recipes)
    }

    pub fn as_ref(&self) -> &HashMap<String, Recipe> {
        &self.0
    }

    pub fn as_ref_mut(&mut self) -> &mut HashMap<String, Recipe> {
        &mut self.0
    }
}

#[derive(Debug)]
pub struct Recipe {
    pub metadata: Metadata,
    pub build: Build,
    pub env: Option<toml::value::Table>,
}

impl TryFrom<RecipeRep> for Recipe {
    type Error = Error;

    fn try_from(rep: RecipeRep) -> Result<Self> {
        Ok(Self {
            metadata: Metadata::try_from(rep.metadata)?,
            build: Build::try_from(rep.build)?,
            env: rep.env,
        })
    }
}

#[derive(Deserialize, Debug)]
pub struct RecipeRep {
    pub metadata: MetadataRep,
    pub build: BuildRep,
    pub env: Option<toml::value::Table>,
}

impl RecipeRep {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(toml::from_slice::<RecipeRep>(&fs::read(&path)?)?)
    }
}

impl TryFrom<DirEntry> for RecipeRep {
    type Error = Error;

    fn try_from(entry: DirEntry) -> Result<Self> {
        let mut path = entry.path();
        path.push(DEFAULT_RECIPE_FILE);
        RecipeRep::new(path)
    }
}

#[derive(Debug)]
pub struct Metadata {
    // General
    pub name: String,
    pub version: String,
    pub arch: String,
    pub revision: String,
    pub description: String,
    pub license: String,
    pub source: String,
    pub images: Vec<toml::Value>,

    // Git repository as source
    pub git: Option<String>,

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

        Ok(Self {
            name: rep.name,
            version: rep.version,
            arch: rep.arch,
            revision: rep.revision,
            description: rep.description,
            license: rep.license,
            source: rep.source,
            images: rep.images,
            git: rep.git,
            depends,
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

    // Git repository as source
    pub git: Option<String>,

    pub depends: Option<Vec<String>>,
    pub obsoletes: Option<Vec<String>>,
    pub conflicts: Option<Vec<String>>,
    pub provides: Option<Vec<String>>,

    // Directories to exclude when creating the package
    pub exclude: Option<Vec<String>>,

    // Only Debian based
    pub maintainer: Option<String>,
    pub section: Option<String>,
    pub priority: Option<String>,
}

#[derive(Debug)]
pub struct Build {
    pub steps: Vec<Cmd>,
}

impl TryFrom<BuildRep> for Build {
    type Error = Error;

    fn try_from(rep: BuildRep) -> Result<Self> {
        let mut steps = Vec::with_capacity(rep.steps.len());

        for result in rep.steps.into_iter().map(|it| Cmd::new(it.as_str())) {
            steps.push(result?);
        }

        Ok(Self { steps })
    }
}

#[derive(Deserialize, Debug)]
pub struct BuildRep {
    pub steps: Vec<String>,
}
