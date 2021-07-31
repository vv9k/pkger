mod cmd;
mod envs;
mod metadata;

pub use cmd::Command;
pub use envs::Env;
pub use metadata::{
    deserialize_images, BuildTarget, DebInfo, DebRep, Dependencies, Distro, GitSource, ImageTarget,
    Metadata, MetadataRep, Os, PackageManager, Patch, Patches, PkgInfo, PkgRep, RpmInfo, RpmRep,
};

use crate::{Error, Result};

use anyhow::Context;
use deb_control::{binary::BinaryDebControl, DebControlBuilder};
use pkgbuild::PkgBuild;
use rpmspec::RpmSpec;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use std::convert::TryFrom;
use std::fs::{self, DirEntry};
use std::path::Path;
use std::path::PathBuf;
use tracing::{info_span, trace, warn};

const DEFAULT_RECIPE_FILE: &str = "recipe.yml";

#[derive(Clone, Deserialize, Serialize, Debug, Eq, PartialEq, Hash)]
pub struct RecipeTarget {
    name: String,
    image_target: ImageTarget,
}

impl RecipeTarget {
    pub fn new(name: String, image_target: ImageTarget) -> Self {
        Self { name, image_target }
    }

    pub fn build_target(&self) -> &BuildTarget {
        &self.image_target.build_target
    }

    #[allow(dead_code)]
    pub fn recipe(&self) -> &str {
        &self.name
    }

    pub fn image(&self) -> &str {
        &self.image_target.image
    }

    pub fn image_os(&self) -> &Option<Os> {
        &self.image_target.os
    }
}

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
            return Err(Error::msg("recipes path is not a directory"));
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
    pub fn load_all(&self) -> Result<Vec<Recipe>> {
        let path = self.path.as_path();

        let span = info_span!("load-recipes", path = %path.display());
        let _enter = span.enter();
        let mut recipes = Vec::new();

        for entry in fs::read_dir(path)? {
            match entry {
                Ok(entry) => {
                    let filename = entry.file_name().to_string_lossy().to_string();
                    let path = entry.path();
                    match RecipeRep::try_from(entry).map(|rep| Recipe::new(rep, path)) {
                        Ok(result) => {
                            let recipe = result?;
                            trace!(recipe = ?recipe);
                            recipes.push(recipe);
                        }
                        Err(e) => {
                            warn!(recipe = %filename, reason = %format!("{:?}", e), "failed to read recipe")
                        }
                    }
                }
                Err(e) => warn!(reason = %format!("{:?}", e), "invalid entry"),
            }
        }

        Ok(recipes)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub metadata: Metadata,
    pub env: Env,
    pub configure_script: Option<ConfigureScript>,
    pub build_script: BuildScript,
    pub install_script: Option<InstallScript>,
    pub recipe_dir: PathBuf,
}

impl Recipe {
    pub fn new(rep: RecipeRep, recipe_dir: PathBuf) -> Result<Self> {
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
            recipe_dir,
        })
    }
}

impl Recipe {
    pub fn as_deb_control(&self, image: &str, installed_size: Option<&str>) -> BinaryDebControl {
        let name = if self.metadata.name.contains('_') {
            warn!("Debian package names can't contain `_`, converting to `-`");
            self.metadata.name.replace('_', "-")
        } else {
            self.metadata.name.to_owned()
        };

        let mut builder = DebControlBuilder::binary_package_builder(&name)
            .version(&self.metadata.version)
            .revision(self.metadata.release())
            .description(&self.metadata.description)
            .architecture(self.metadata.arch.deb_name());

        if let Some(epoch) = &self.metadata.epoch {
            builder = builder.epoch(epoch);
        }
        if let Some(group) = &self.metadata.group {
            builder = builder.section(group);
        }
        if let Some(depends) = &self.metadata.depends {
            builder = builder.add_depends_entries(depends.resolve_names(image));
        }
        if let Some(conflicts) = &self.metadata.conflicts {
            builder = builder.add_conflicts_entries(conflicts.resolve_names(image));
        }
        if let Some(provides) = &self.metadata.provides {
            builder = builder.add_provides_entries(provides.resolve_names(image));
        }
        if let Some(maintainer) = &self.metadata.maintainer {
            builder = builder.maintainer(maintainer);
        }
        if let Some(homepage) = &self.metadata.url {
            builder = builder.homepage(homepage);
        }
        if let Some(installed_size) = installed_size {
            builder = builder.installed_size(installed_size)
        }

        if let Some(deb) = &self.metadata.deb {
            if let Some(priority) = &deb.priority {
                builder = builder.priority(priority);
            }
            if let Some(built_using) = &deb.built_using {
                builder = builder.built_using(built_using);
            }
            if let Some(essential) = &deb.essential {
                builder = builder.essential(*essential);
            }

            if let Some(pre_depends) = &deb.pre_depends {
                builder = builder.add_pre_depends_entries(pre_depends.resolve_names(image));
            }
            if let Some(recommends) = &deb.recommends {
                builder = builder.add_recommends_entries(recommends.resolve_names(image));
            }
            if let Some(suggests) = &deb.suggests {
                builder = builder.add_suggests_entries(suggests.resolve_names(image));
            }
            if let Some(breaks) = &deb.breaks {
                builder = builder.add_breaks_entries(breaks.resolve_names(image));
            }
            if let Some(replaces) = &deb.replaces {
                builder = builder.add_replaces_entries(replaces.resolve_names(image));
            }
            if let Some(enchances) = &deb.enhances {
                builder = builder.add_enchances_entries(enchances.resolve_names(image));
            }
        }

        builder.build()
    }

    pub fn as_rpm_spec(&self, sources: &[String], files: &[String], image: &str) -> RpmSpec {
        let install_script = sources
            .iter()
            .enumerate()
            .fold(String::new(), |mut s, (i, _)| {
                s.push_str(&format!("tar xvf %{{SOURCE{}}} -C %{{buildroot}}\n", i));
                s
            });

        let mut builder = RpmSpec::builder()
            .name(&self.metadata.name)
            .build_arch(self.metadata.arch.rpm_name())
            .description(&self.metadata.description)
            .license(&self.metadata.license)
            .version(&self.metadata.version)
            .release(self.metadata.release())
            .add_files_entries(files)
            .add_sources_entries(sources)
            .add_macro("__os_install_post", None::<&str>, "%{nil}") // disable binary stripping
            .install_script(&install_script)
            .description(&self.metadata.description);

        if let Some(rpm) = &self.metadata.rpm {
            if let Some(obsoletes) = &rpm.obsoletes {
                builder = builder.add_obsoletes_entries(obsoletes.resolve_names(image));
            }
            if let Some(vendor) = &rpm.vendor {
                builder = builder.vendor(vendor);
            }
            if let Some(icon) = &rpm.icon {
                builder = builder.icon(icon);
            }
            if let Some(pre_script) = &rpm.pre_script {
                builder = builder.pre_script(pre_script);
            }
            if let Some(post_script) = &rpm.post_script {
                builder = builder.post_script(post_script);
            }
            if let Some(preun_script) = &rpm.preun_script {
                builder = builder.preun_script(preun_script);
            }
            if let Some(post_script) = &rpm.post_script {
                builder = builder.post_script(post_script);
            }
            if let Some(config_noreplace) = &rpm.config_noreplace {
                builder = builder.config_noreplace(config_noreplace);
            }
            if let Some(summary) = &rpm.summary {
                builder = builder.summary(summary);
            } else {
                builder = builder.summary(&self.metadata.description);
            }
        }
        if let Some(group) = &self.metadata.group {
            builder = builder.group(group);
        }
        if let Some(maintainer) = &self.metadata.maintainer {
            builder = builder.packager(maintainer);
        }
        if let Some(url) = &self.metadata.url {
            builder = builder.url(url);
        }
        if let Some(epoch) = &self.metadata.epoch {
            builder = builder.epoch(epoch);
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

        builder.build()
    }

    pub fn as_pkgbuild(&self, image: &str, sources: &[String], checksums: &[String]) -> PkgBuild {
        let package_func = sources.iter().fold(String::new(), |mut s, src| {
            s.push_str(&format!("    tar xvf {} -C $pkgdir\n", src));
            s
        });

        let mut builder = PkgBuild::builder()
            .pkgname(&self.metadata.name)
            .pkgver(&self.metadata.version)
            .pkgdesc(&self.metadata.description)
            .add_license_entries(vec![&self.metadata.license])
            .add_arch_entries(vec![self.metadata.arch.pkg_name().to_string()])
            .add_source_entries(sources)
            .add_md5sums_entries(checksums)
            .package_func(package_func);

        if let Some(url) = &self.metadata.url {
            builder = builder.url(url);
        }
        if let Some(group) = &self.metadata.group {
            builder = builder.add_groups_entries(vec![group]);
        }
        if let Some(depends) = &self.metadata.depends {
            builder = builder.add_depends_entries(depends.resolve_names(image));
        }
        if let Some(conflicts) = &self.metadata.conflicts {
            builder = builder.add_conflicts_entries(conflicts.resolve_names(image));
        }
        if let Some(provides) = &self.metadata.provides {
            builder = builder.add_provides_entries(provides.resolve_names(image));
        }

        builder = builder.pkgrel(self.metadata.release());

        builder.build()
    }
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct RecipeRep {
    pub metadata: MetadataRep,
    pub env: Option<Mapping>,
    pub configure: Option<ConfigureRep>,
    pub build: BuildRep,
    pub install: Option<InstallRep>,
}

impl RecipeRep {
    pub fn from_yaml_bytes(data: &[u8]) -> Result<Self> {
        Ok(serde_yaml::from_slice(data)?)
    }

    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::from_yaml_bytes(&fs::read(&path)?)
    }
}

impl TryFrom<DirEntry> for RecipeRep {
    type Error = Error;

    fn try_from(entry: DirEntry) -> Result<Self> {
        let mut path = entry.path();
        path.push(DEFAULT_RECIPE_FILE);
        RecipeRep::load(path)
    }
}

macro_rules! impl_step_rep {
    ($ty:ident, $ty_rep:ident) => {
        #[derive(Clone, Debug, PartialEq)]
        pub struct $ty {
            pub steps: Vec<Command>,
            pub working_dir: Option<PathBuf>,
            pub shell: Option<String>,
        }

        impl TryFrom<$ty_rep> for $ty {
            type Error = Error;

            fn try_from(rep: $ty_rep) -> Result<Self> {
                Ok(Self {
                    steps: rep.steps,
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
            pub steps: Vec<Command>,
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

    const TEST_RECIPE: &[u8] = include_bytes!("../../../example/recipes/test-suite/recipe.yml");

    #[test]
    fn parses_recipe_from_rep() {
        let rep = RecipeRep::from_yaml_bytes(TEST_RECIPE).unwrap();
        let parsed = Recipe::new(rep.clone(), PathBuf::new()).unwrap();

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
