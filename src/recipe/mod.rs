mod envs;
mod metadata;

pub use envs::Env;
pub use metadata::{BuildTarget, GitSource, ImageTarget, Metadata, MetadataRep};

use crate::cmd::Cmd;
use crate::{Error, Result};

use deb_control::{binary::BinaryDebControl, DebControlBuilder};
use rpmspec::RpmSpec;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::fs::{self, DirEntry};
use std::path::Path;
use std::{collections::HashMap, path::PathBuf};
use tracing::{info_span, trace, warn};

const DEFAULT_RECIPE_FILE: &str = "recipe.toml";

#[derive(Clone, Deserialize, Serialize, Debug, Eq, PartialEq, Hash)]
pub struct RecipeTarget {
    name: String,
    image_target: ImageTarget,
}

impl RecipeTarget {
    pub fn new(name: String, image_target: ImageTarget) -> Self {
        Self { name, image_target }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Recipes {
    inner: HashMap<String, Recipe>,
    path: PathBuf,
}

impl Recipes {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Recipes {
            path: path.as_ref().to_path_buf(),
            ..Default::default()
        }
    }

    pub fn load(&mut self) -> Result<()> {
        let path = self.path.as_path();

        let span = info_span!("load-recipes", path = %path.display());
        let _enter = span.enter();

        if !path.is_dir() {
            return Err(anyhow!("recipes path is not a directory"));
        }

        for entry in fs::read_dir(path)? {
            match entry {
                Ok(entry) => {
                    let filename = entry.file_name().to_string_lossy().to_string();
                    match RecipeRep::try_from(entry).map(Recipe::try_from) {
                        Ok(result) => {
                            let recipe = result?;
                            trace!(recipe = ?recipe);
                            self.inner.insert(filename, recipe);
                        }
                        Err(e) => warn!(recipe = %filename, reason = %e, "failed to read recipe"),
                    }
                }
                Err(e) => warn!(reason = %e, "invalid entry"),
            }
        }

        Ok(())
    }

    pub fn inner_ref(&self) -> &HashMap<String, Recipe> {
        &self.inner
    }

    pub fn inner_ref_mut(&mut self) -> &mut HashMap<String, Recipe> {
        &mut self.inner
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

impl Recipe {
    pub fn as_deb_control(&self, image: &str) -> BinaryDebControl {
        let arch = self.metadata.deb_arch();
        let maintainer = if let Some(maintainer) = &self.metadata.maintainer {
            maintainer
        } else {
            "none"
        };
        let mut builder = DebControlBuilder::binary_package_builder(&self.metadata.name)
            .version(&self.metadata.version)
            .description(&self.metadata.description)
            .maintainer(maintainer)
            .architecture(arch);

        if let Some(depends) = &self.metadata.depends {
            builder = builder.add_depends_entries(depends.resolve_names(image));
        }
        if let Some(conflicts) = &self.metadata.conflicts {
            builder = builder.add_conflicts_entries(conflicts.resolve_names(image));
        }
        if let Some(provides) = &self.metadata.provides {
            builder = builder.add_provides_entries(provides.resolve_names(image));
        }

        if let Some(group) = &self.metadata.group {
            builder = builder.section(group);
        }

        builder.build()
    }

    pub fn as_rpm_spec(
        &self,
        sources: &[String],
        files: &[String],
        dirs: &[String],
        image: &str,
    ) -> RpmSpec {
        let install_script = sources
            .iter()
            .enumerate()
            .fold(String::new(), |mut s, (i, _)| {
                s.push_str(&format!("tar xvf %{{SOURCE{}}} -C %{{buildroot}}\n", i));
                s
            });

        macro_rules! let_some_or {
            ($field:ident, $default:expr) => {
                if let Some($field) = &self.metadata.$field {
                    $field
                } else {
                    $default
                };
            };
        }
        let release = let_some_or!(release, "0");
        let arch = let_some_or!(arch, "noarch");
        let summary = let_some_or!(summary, &self.metadata.description);

        let mut builder = RpmSpec::builder()
            .name(&self.metadata.name)
            .build_arch(arch)
            .summary(summary)
            .description(&self.metadata.description)
            .license(&self.metadata.license)
            .version(&self.metadata.version)
            .release(release)
            .add_files_entries(files)
            .add_dir_files_entries(dirs)
            .add_sources_entries(sources)
            .install_script(&install_script)
            .description(&self.metadata.description);
        if let Some(group) = &self.metadata.group {
            builder = builder.group(group);
        }
        if let Some(maintainer) = &self.metadata.maintainer {
            builder = builder.packager(maintainer);
        }
        if let Some(epoch) = &self.metadata.epoch {
            builder = builder.epoch(epoch);
        }
        if let Some(vendor) = &self.metadata.vendor {
            builder = builder.vendor(vendor);
        }
        if let Some(icon) = &self.metadata.icon {
            builder = builder.icon(icon);
        }
        if let Some(pre_script) = &self.metadata.pre_script {
            builder = builder.pre_script(pre_script);
        }
        if let Some(post_script) = &self.metadata.post_script {
            builder = builder.post_script(post_script);
        }
        if let Some(preun_script) = &self.metadata.preun_script {
            builder = builder.preun_script(preun_script);
        }
        if let Some(post_script) = &self.metadata.post_script {
            builder = builder.post_script(post_script);
        }
        if let Some(config_noreplace) = &self.metadata.config_noreplace {
            builder = builder.config_noreplace(config_noreplace);
        }
        if let Some(conflicts) = &self.metadata.conflicts {
            builder = builder.add_conflicts_entries(conflicts.resolve_names(image));
        }
        if let Some(provides) = &self.metadata.provides {
            builder = builder.add_provides_entries(provides.resolve_names(image));
        }
        if let Some(requires) = &self.metadata.depends {
            builder = builder.add_requires_entries(requires.resolve_names(image));
        }
        if let Some(obsoletes) = &self.metadata.obsoletes {
            builder = builder.add_obsoletes_entries(obsoletes.resolve_names(image));
        }

        builder.build()
    }
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct RecipeRep {
    pub metadata: MetadataRep,
    pub env: Option<toml::value::Table>,
    pub configure: Option<ConfigureRep>,
    pub build: BuildRep,
    pub install: Option<InstallRep>,
}

impl RecipeRep {
    pub fn from_toml_bytes(data: &[u8]) -> Result<Self> {
        Ok(toml::from_slice(&data)?)
    }

    pub fn read_from<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::from_toml_bytes(&fs::read(&path)?)
    }
}

impl TryFrom<DirEntry> for RecipeRep {
    type Error = Error;

    fn try_from(entry: DirEntry) -> Result<Self> {
        let mut path = entry.path();
        path.push(DEFAULT_RECIPE_FILE);
        RecipeRep::read_from(path)
    }
}

macro_rules! impl_step_rep {
    ($ty:ident, $ty_rep:ident) => {
        #[derive(Clone, Debug)]
        pub struct $ty {
            pub steps: Vec<Cmd>,
            pub working_dir: Option<PathBuf>,
            pub shell: Option<String>,
        }

        impl TryFrom<$ty_rep> for $ty {
            type Error = Error;

            fn try_from(rep: $ty_rep) -> Result<Self> {
                let mut steps = Vec::with_capacity(rep.steps.len());

                for result in rep.steps.into_iter().map(Cmd::try_from) {
                    steps.push(result?);
                }

                Ok(Self {
                    steps,
                    working_dir: rep.working_dir,
                    shell: rep.shell,
                })
            }
        }

        impl $ty {
            #[allow(dead_code)]
            pub fn steps_as_script(&self) -> String {
                let mut script = String::new();
                self.steps.iter().for_each(|step| {
                    script.push_str(&step.cmd);
                    script.push('\n');
                });
                script
            }
        }

        #[derive(Clone, Deserialize, Serialize, Debug, Default)]
        pub struct $ty_rep {
            pub steps: Vec<toml::Value>,
            pub working_dir: Option<PathBuf>,
            pub shell: Option<String>,
        }
    };
}

impl_step_rep!(BuildScript, BuildRep);
impl_step_rep!(InstallScript, InstallRep);
impl_step_rep!(ConfigureScript, ConfigureRep);

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const TEST_RECIPE: &[u8] = include_bytes!("../../example/recipes/test/recipe.toml");

    #[test]
    fn parses_recipe_from_rep() {
        let rep = RecipeRep::from_toml_bytes(&TEST_RECIPE).unwrap();
        let parsed = Recipe::try_from(rep.clone()).unwrap();

        let rep_config = rep.configure.unwrap();
        let config = parsed.configure_script.unwrap();
        assert_eq!(config.working_dir, rep_config.working_dir);
        assert_eq!(config.shell, rep_config.shell);

        assert_eq!(parsed.build_script.working_dir, rep.build.working_dir);
        assert_eq!(parsed.build_script.shell, rep.build.shell);

        let rep_install = rep.install.unwrap();
        let install = parsed.install_script.unwrap();
        assert_eq!(install.working_dir, rep_install.working_dir);
        assert_eq!(install.shell, rep_install.shell);
    }
}
