mod cmd;
mod envs;
mod loader;
mod metadata;
mod target;

pub use cmd::Command;
pub use envs::Env;
pub use loader::Loader;
pub use metadata::{
    deserialize_images, BuildArch, BuildTarget, DebInfo, DebRep, Dependencies, Distro, GitSource,
    ImageTarget, Metadata, MetadataRep, Os, PackageManager, Patch, Patches, PkgInfo, PkgRep,
    RpmInfo, RpmRep,
};
pub use target::RecipeTarget;

use crate::log::{warning, BoxedCollector};
use crate::{err, ErrContext, Error, Result};

use apkbuild::ApkBuild;
use debbuild::{binary::BinaryDebControl, DebControlBuilder};
use merge_yaml_hash::MergeYamlHash;
use pkgbuild::PkgBuild;
use rpmspec::RpmSpec;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use std::convert::TryFrom;
use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};

const DEFAULT_RECIPE_FILE: &str = "recipe.yml";

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
    pub fn new(mut rep: RecipeRep, recipe_dir: PathBuf) -> Result<Self> {
        let is_inherited = match (&rep.metadata, &rep.build, &rep.from) {
            (Some(_), None, None)
            | (None, Some(_), None)
            | (None, None, None)
            | (None, None, Some(_))
            | (None, Some(_), Some(_)) => {
                return err!("invalid recipe, must either contain a `metadata` section with a name and a 'from' reference to other recipe or `metadata` and `build` section");
            }
            (Some(metadata), _, Some(_)) if metadata.name.is_none() => {
                return err!("invalid recipe, must either contain a `metadata` section with a name and a 'from' reference to other recipe or `metadata` and `build` section");
            }
            (Some(_), Some(_), None) => false,
            (Some(_), None, Some(_)) | (Some(_), Some(_), Some(_)) => true,
        };

        match (&rep.metadata, is_inherited) {
            (Some(metadata), false) if metadata.description.is_none() => {
                return err!("invalid recipe, it's a base recipe and has no description specified");
            }
            (Some(metadata), false) if metadata.license.is_none() => {
                return err!("invalid recipe, it's a base recipe and has no license specified");
            }
            _ => {}
        }

        if is_inherited {
            if let Some(dir) = recipe_dir.parent() {
                let loader = Loader::new(dir)?;
                let base_rep = loader
                    .load_rep(rep.from.as_ref().unwrap())
                    .context("failed to load base recipe")?;
                rep = rep.merge(base_rep).context("failed to merge recipes")?;
            } else {
                return err!("failed to determine recipes directory");
            }
        }

        Ok(Self {
            metadata: Metadata::try_from(
                rep.metadata
                    .ok_or_else(|| Error::msg("invalid recipe, `metadata` section required"))?,
            )?,
            env: Env::from(rep.env),
            configure_script: if let Some(script) = rep.configure {
                Some(ConfigureScript::try_from(script)?)
            } else {
                None
            },
            build_script: BuildScript::try_from(
                rep.build
                    .ok_or_else(|| Error::msg("invalid recipe, `build` section required"))?,
            )?,
            install_script: if let Some(script) = rep.install {
                Some(InstallScript::try_from(script)?)
            } else {
                None
            },
            recipe_dir,
        })
    }

    #[inline]
    pub fn images(&self) -> &[String] {
        &self.metadata.images
    }
}

impl Recipe {
    pub fn as_deb_control(
        &self,
        image: &str,
        installed_size: Option<&str>,
        version: &str,
        logger: &mut BoxedCollector,
    ) -> BinaryDebControl {
        let name = if self.metadata.name.contains('_') {
            warning!(logger => "Debian package names can't contain `_`, converting to `-`");
            self.metadata.name.replace('_', "-")
        } else {
            self.metadata.name.to_owned()
        };

        let mut builder = DebControlBuilder::binary_package_builder(&name)
            .version(version)
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

    pub fn as_rpm_spec(
        &self,
        sources: &[String],
        files: &[String],
        image: &str,
        version: &str,
        _logger: &mut BoxedCollector,
    ) -> RpmSpec {
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
            .version(version)
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

            if !rpm.auto_req_prov {
                builder = builder.disable_auto_req_prov();
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
        } else {
            builder = builder.summary(&self.metadata.description);
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

    pub fn as_pkgbuild(
        &self,
        image: &str,
        sources: &[String],
        checksums: &[String],
        version: &str,
        _logger: &mut BoxedCollector,
    ) -> PkgBuild {
        let package_func = sources.iter().fold(String::new(), |mut s, src| {
            s.push_str(&format!("    tar xvf {} -C $pkgdir\n", src));
            s
        });

        let mut builder = PkgBuild::builder()
            .pkgname(&self.metadata.name)
            .pkgver(version)
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

    pub fn as_apkbuild(
        &self,
        image: &str,
        sources: &[String],
        builddir: &Path,
        version: &str,
        _logger: &mut BoxedCollector,
    ) -> ApkBuild {
        let package_func =
            sources
                .iter()
                .fold("    mkdir -p $pkgdir\n".to_string(), |mut s, src| {
                    s.push_str(&format!("    tar xvf {} -C $pkgdir\n", src));
                    s
                });

        let mut builder = ApkBuild::builder()
            .pkgname(&self.metadata.name)
            .pkgver(version)
            .pkgdesc(&self.metadata.description)
            .add_license_entries(vec![&self.metadata.license])
            .add_arch_entries(vec![self.metadata.arch.apk_name().to_string()])
            .add_source_entries(sources)
            .package_func(package_func)
            .builddir(builddir.to_string_lossy());

        builder = builder.url(self.metadata.url.as_deref().unwrap_or(" "));
        if let Some(depends) = &self.metadata.depends {
            builder = builder.add_depends_entries(depends.resolve_names(image));
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MetadataRep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Mapping>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configure: Option<ConfigureRep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildRep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<InstallRep>,
}

impl RecipeRep {
    pub fn from_yaml_bytes(data: &[u8]) -> Result<Self> {
        Ok(serde_yaml::from_slice(data)?)
    }

    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::from_yaml_bytes(&fs::read(&path)?)
    }

    pub(crate) fn merge(self, base_rep: RecipeRep) -> Result<RecipeRep> {
        let base_value =
            serde_yaml::to_string(&base_rep).context("failed to serialize base recipe")?;
        let rep_value = serde_yaml::to_string(&self).context("failed to serialize recipe")?;

        let mut merged = MergeYamlHash::new();
        merged.merge(&base_value);
        merged.merge(&rep_value);

        serde_yaml::from_str(&merged.to_string()).context("failed to deserialize merged recipe")
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
            pub fn steps_as_script(&self) -> String {
                let mut script = String::new();
                self.steps.iter().for_each(|step| {
                    script.push_str(&step.cmd);
                    script.push('\n');
                });
                script
            }
        }

        #[derive(Clone, Deserialize, Serialize, Debug, Default, PartialEq)]
        pub struct $ty_rep {
            pub steps: Vec<Command>,
            #[serde(skip_serializing_if = "Option::is_none")]
            pub working_dir: Option<PathBuf>,
            #[serde(skip_serializing_if = "Option::is_none")]
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

    const TEST_SUITE_RECIPE: &[u8] =
        include_bytes!("../../../example/recipes/test-suite/recipe.yml");
    const BASE_RECIPE: &[u8] = include_bytes!("../../../example/recipes/base-package/recipe.yml");
    const CHILD1_RECIPE: &[u8] =
        include_bytes!("../../../example/recipes/child-package1/recipe.yml");
    const CHILD2_RECIPE: &[u8] =
        include_bytes!("../../../example/recipes/child-package2/recipe.yml");

    #[test]
    fn parses_recipe_from_rep() {
        let rep = RecipeRep::from_yaml_bytes(TEST_SUITE_RECIPE).unwrap();
        let parsed = Recipe::new(rep.clone(), PathBuf::new()).unwrap();

        let rep_config = rep.configure.unwrap();
        let config = parsed.configure_script.unwrap();
        assert_eq!(config.working_dir, rep_config.working_dir);
        assert_eq!(config.shell, rep_config.shell);

        let build = rep.build.unwrap();
        assert_eq!(parsed.build_script.working_dir, build.working_dir);
        assert_eq!(parsed.build_script.shell, build.shell);

        let rep_install = rep.install.unwrap();
        let install = parsed.install_script.unwrap();
        assert_eq!(install.working_dir, rep_install.working_dir);
        assert_eq!(install.shell, rep_install.shell);
    }

    #[test]
    fn merges_base_recipe_with_child() {
        let base_rep = RecipeRep::from_yaml_bytes(BASE_RECIPE).unwrap();
        let base_metadata = base_rep.metadata.clone().unwrap();

        let child1_rep = RecipeRep::from_yaml_bytes(CHILD1_RECIPE).unwrap();
        let child1_rep_merged = child1_rep.clone().merge(base_rep.clone()).unwrap();
        let metadata_before = child1_rep.metadata.unwrap();
        let metadata_merged = child1_rep_merged.metadata.unwrap();

        assert_eq!(metadata_merged.name, metadata_before.name);
        assert_eq!(metadata_merged.version, metadata_before.version);
        assert_eq!(metadata_merged.description, metadata_before.description);
        assert_eq!(metadata_merged.license, base_metadata.license);
        assert_eq!(metadata_merged.images, base_metadata.images);
        assert_eq!(child1_rep_merged.build, base_rep.build);
        assert_eq!(child1_rep_merged.build, base_rep.build);

        let child2_rep = RecipeRep::from_yaml_bytes(CHILD2_RECIPE).unwrap();
        let child2_rep_merged = child2_rep.clone().merge(base_rep.clone()).unwrap();
        let metadata_before = child2_rep.metadata.unwrap();
        let metadata_merged = child2_rep_merged.metadata.unwrap();

        assert_eq!(metadata_merged.name, metadata_before.name);
        assert_eq!(metadata_merged.version, metadata_before.version);
        assert_eq!(metadata_merged.description, metadata_before.description);
        assert_eq!(metadata_merged.license, base_metadata.license);
        assert_eq!(metadata_merged.images, base_metadata.images);
        assert_eq!(
            child2_rep_merged.build.as_ref().map(|b| b.steps.clone()),
            child2_rep.build.as_ref().map(|b| b.steps.clone())
        );
        assert_eq!(
            child2_rep_merged
                .build
                .as_ref()
                .map(|b| b.working_dir.clone()),
            base_rep.build.as_ref().map(|b| b.working_dir.clone())
        );
    }

    #[test]
    fn invalid_recipes() {
        let recipe = r#"
from: base
build:
  steps: []"#;
        let recipe: serde_yaml::Value = serde_yaml::from_str(recipe).unwrap();

        let rep = RecipeRep::from_yaml_bytes(&serde_yaml::to_vec(&recipe).unwrap()).unwrap();
        let res = Recipe::new(rep, PathBuf::new());
        println!("\n\n\n\n\n\n\n{:?}", res);
        assert!(res.is_err());
        let recipe = r#"
from: base
metadata:
  version: "1.2.3"
build:
  steps: []"#;
        let recipe: serde_yaml::Value = serde_yaml::from_str(recipe).unwrap();

        let rep = RecipeRep::from_yaml_bytes(&serde_yaml::to_vec(&recipe).unwrap()).unwrap();
        let res = Recipe::new(rep, PathBuf::new());
        println!("\n\n\n\n\n\n\n{:?}", res);
        assert!(res.is_err());
    }
}
