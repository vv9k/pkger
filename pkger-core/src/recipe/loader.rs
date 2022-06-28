use crate::log::{debug, trace, warning, BoxedCollector};
use crate::recipe::{Recipe, RecipeRep};
use crate::{err, ErrContext, Error, Result};

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Loader {
    path: PathBuf,
}

impl Loader {
    /// Initializes a recipe loader without loading any recipes. The provided `path` must be a directory
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let metadata = fs::metadata(path)
            .context(format!("failed to verify recipe path `{}`", path.display()))?;

        if !metadata.is_dir() {
            return err!("recipes path is not a directory");
        }

        Ok(Loader {
            path: path.to_path_buf(),
        })
    }

    pub fn load(&self, recipe: &str) -> Result<Recipe> {
        let base_path = self.path.join(recipe);
        let mut path = base_path.join("recipe.yml");
        if !path.exists() {
            path = base_path.join("recipe.yaml");
        }
        RecipeRep::load(path).and_then(|rep| Recipe::new(rep, base_path))
    }

    pub fn list(&self) -> Result<Vec<String>> {
        fs::read_dir(&self.path)
            .map(|entries| {
                entries
                    .filter_map(|entry| {
                        entry
                            .ok()
                            .filter(|e| e.file_type().map(|e| e.is_dir()).unwrap_or(false))
                            .map(|e| e.file_name().to_string_lossy().to_string())
                    })
                    .collect()
            })
            .context("failed to list recipes")
    }

    /// Loads all recipes in the underlying directory
    pub fn load_all(&self, logger: &mut BoxedCollector) -> Result<Vec<Recipe>> {
        let path = self.path.as_path();

        debug!(logger => "loading reicipes from '{}'", path.display());

        let mut recipes = Vec::new();

        for entry in fs::read_dir(path)? {
            match entry {
                Ok(entry) => {
                    let filename = entry.file_name().to_string_lossy().to_string();
                    let path = entry.path();
                    match RecipeRep::try_from(entry).map(|rep| Recipe::new(rep, path)) {
                        Ok(result) => {
                            let recipe = result?;
                            trace!(logger => "{:?}", recipe);
                            recipes.push(recipe);
                        }
                        Err(e) => {
                            warning!(logger => "failed to read recipe from '{}', reason: {:?}", filename, e);
                        }
                    }
                }
                Err(e) => warning!(logger => "invalid entry, reason: {:?}", e),
            }
        }

        Ok(recipes)
    }
}
