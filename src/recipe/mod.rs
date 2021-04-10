mod envs;
mod metadata;

pub use envs::Env;
pub use metadata::{Metadata, MetadataRep};

use crate::cmd::Cmd;
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
    pub env: Env,
}

impl TryFrom<RecipeRep> for Recipe {
    type Error = Error;

    fn try_from(rep: RecipeRep) -> Result<Self> {
        Ok(Self {
            metadata: Metadata::try_from(rep.metadata)?,
            build: Build::try_from(rep.build)?,
            env: Env::from(rep.env),
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
