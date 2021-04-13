mod envs;
mod metadata;

use deb_control::{binary::BinaryDebControl, DebControlBuilder};
pub use envs::Env;
pub use metadata::{BuildTarget, Metadata, MetadataRep};
use rpmspec::RpmSpec;

use crate::cmd::Cmd;
use crate::{Error, Result};

use serde::Deserialize;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::env;
use std::fs::{self, DirEntry};
use std::path::Path;
use tracing::error;

const DEFAULT_RECIPE_FILE: &str = "recipe.toml";

#[derive(Clone, Debug, Default)]
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

    pub fn inner_ref(&self) -> &HashMap<String, Recipe> {
        &self.0
    }

    pub fn inner_ref_mut(&mut self) -> &mut HashMap<String, Recipe> {
        &mut self.0
    }
}

#[derive(Clone, Debug)]
pub struct Recipe {
    pub metadata: Metadata,
    pub env: Env,
    pub configure_script: Option<ConfigureScript>,
    pub build_script: BuildScript,
    pub install_script: Option<InstallScript>,
}

impl TryFrom<RecipeRep> for Recipe {
    type Error = Error;

    fn try_from(rep: RecipeRep) -> Result<Self> {
        Ok(Self {
            metadata: Metadata::try_from(rep.metadata)?,
            env: Env::from(rep.env),
            configure_script: if let Some(script) = rep.configure {
                Some(ConfigureScript::try_from(script)?)
            } else {
                None
            },
            build_script: BuildScript::try_from(rep.build)?,
            install_script: if let Some(script) = rep.install {
                Some(InstallScript::try_from(script)?)
            } else {
                None
            },
        })
    }
}

impl From<&Recipe> for RpmSpec {
    fn from(recipe: &Recipe) -> Self {
        let mut builder = RpmSpec::builder()
            .name(&recipe.metadata.name)
            .license(&recipe.metadata.license)
            .version(&recipe.metadata.version)
            .release(&recipe.metadata.revision)
            .description(&recipe.metadata.description)
            .build_script(&recipe.build_script.steps_as_script());

        if let Some(config) = &recipe.configure_script {
            builder = builder.prep_script(config.steps_as_script());
        }

        if let Some(install) = &recipe.install_script {
            builder = builder.install_script(install.steps_as_script());
        }

        builder.build()
    }
}

impl From<&Recipe> for BinaryDebControl {
    fn from(recipe: &Recipe) -> Self {
        DebControlBuilder::binary_package_builder(&recipe.metadata.name)
            .version(&recipe.metadata.version)
            .description(&recipe.metadata.description)
            .build()
    }
}

#[derive(Deserialize, Debug)]
pub struct RecipeRep {
    pub metadata: MetadataRep,
    pub env: Option<toml::value::Table>,
    pub configure: Option<ConfigureRep>,
    pub build: BuildRep,
    pub install: Option<InstallRep>,
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

macro_rules! impl_step_rep {
    ($ty:ident, $ty_rep:ident) => {
        #[derive(Clone, Debug)]
        pub struct $ty {
            pub steps: Vec<Cmd>,
        }

        impl TryFrom<$ty_rep> for $ty {
            type Error = Error;

            fn try_from(rep: $ty_rep) -> Result<Self> {
                let mut steps = Vec::with_capacity(rep.steps.len());

                for result in rep.steps.into_iter().map(|it| Cmd::new(it.as_str())) {
                    steps.push(result?);
                }

                Ok(Self { steps })
            }
        }

        impl $ty {
            pub fn steps_as_script(&self) -> String {
                let mut script = String::new();
                self.steps.iter().for_each(|step| {
                    script.push_str(&step.cmd);
                    script.push('\n');
                });
                script
            }
        }

        #[derive(Deserialize, Debug)]
        pub struct $ty_rep {
            pub steps: Vec<String>,
        }
    };
}

impl_step_rep!(BuildScript, BuildRep);
impl_step_rep!(InstallScript, InstallRep);
impl_step_rep!(ConfigureScript, ConfigureRep);
