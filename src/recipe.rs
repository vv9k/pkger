use crate::{Error, Result};

use log::error;
use serde::Deserialize;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::env;
use std::fs::{self, DirEntry};
use std::path::Path;

pub const DEFAULT_RECIPE_FILE: &str = "recipe.toml";

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
                    match Recipe::try_from(entry) {
                        Ok(recipe) => {
                            recipes.0.insert(filename, recipe);
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

#[derive(Deserialize, Debug)]
pub struct Recipe {
    pub metadata: Metadata,
    pub build: Build,
    pub env: Option<toml::value::Table>,
}

impl Recipe {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(toml::from_slice::<Recipe>(&fs::read(&path)?)?)
    }
}

impl TryFrom<DirEntry> for Recipe {
    type Error = Error;

    fn try_from(entry: DirEntry) -> Result<Self> {
        let mut path = entry.path();
        path.push(DEFAULT_RECIPE_FILE);
        Recipe::new(path)
    }
}

#[derive(Deserialize, Debug)]
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

    // Debian based specific packages
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

#[derive(Deserialize, Debug)]
pub struct Build {
    pub steps: Vec<String>,
}
