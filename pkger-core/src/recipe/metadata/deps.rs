use crate::Result;

use anyhow::Context;
use serde_yaml::{Mapping, Sequence, Value as YamlValue};
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

pub static COMMON_DEPS_KEY: &str = "all";

type DepsMap = HashMap<String, HashSet<String>>;

#[derive(Clone, Debug, PartialEq)]
pub struct Dependencies {
    inner: DepsMap,
}

impl Default for Dependencies {
    fn default() -> Self {
        let mut deps = Self {
            inner: HashMap::new(),
        };

        // ensure the COMMON_DEPS_KEY entry is created by default
        deps.inner
            .insert(COMMON_DEPS_KEY.to_string(), HashSet::new());
        deps
    }
}

impl TryFrom<Mapping> for Dependencies {
    type Error = crate::Error;

    fn try_from(table: Mapping) -> Result<Self, Self::Error> {
        let mut deps = Self::default();
        for (image, image_deps) in table {
            if image_deps.is_sequence() {
                let mut deps_set = HashSet::new();
                for dep in image_deps.as_sequence().unwrap() {
                    if !dep.is_string() {
                        return Err(anyhow!(
                            "expected a string as dependency, found `{:?}`",
                            dep
                        ));
                    }

                    deps_set.insert(dep.as_str().unwrap().to_string());
                }
                let image = image
                    .as_str()
                    .map(|s| s.to_string())
                    .context("expected image name")?;
                if image.contains('+') {
                    for image in image.split('+') {
                        deps.update_or_insert(image.to_string(), &deps_set);
                    }
                } else {
                    deps.update_or_insert(image.to_string(), &deps_set);
                }
            } else {
                return Err(anyhow!(
                    "expected array of dependencies, found `{:?}`",
                    image_deps
                ));
            }
        }
        Ok(deps)
    }
}

impl TryFrom<Sequence> for Dependencies {
    type Error = crate::Error;
    fn try_from(array: Sequence) -> Result<Self> {
        let mut deps = Self::default();
        let mut dep_set = HashSet::new();
        for dep in array {
            if let YamlValue::String(dep) = dep {
                dep_set.insert(dep);
            } else {
                return Err(anyhow!(
                    "expected a string as dependency name, found `{:?}`",
                    dep
                ));
            }
        }
        deps.inner_mut()
            .insert(COMMON_DEPS_KEY.to_string(), dep_set);

        Ok(deps)
    }
}

impl TryFrom<YamlValue> for Dependencies {
    type Error = crate::Error;
    fn try_from(deps: YamlValue) -> Result<Self> {
        match deps {
            YamlValue::Mapping(table) => Self::try_from(table),
            YamlValue::Sequence(array) => Self::try_from(array),
            _ => Err(anyhow!(
                "expected a map or array of dependencies, found `{:?}`",
                deps
            )),
        }
    }
}

impl Dependencies {
    /// Returns a set of dependencies for the given `image`. This includes common images
    /// from [COMMON_DEPS_KEY](COMMON_DEPS_KEY).
    pub fn resolve_names(&self, image: &str) -> HashSet<&str> {
        // it's ok to unwrap here, the new function adds an empty hashset on initialization
        let mut deps = HashSet::new();
        if let Some(common_deps) = self.inner.get(COMMON_DEPS_KEY) {
            deps.extend(common_deps.iter().map(|s| s.as_str()));
        }
        if let Some(image_deps) = self.inner.get(image) {
            deps.extend(image_deps.iter().map(|s| s.as_str()));
        }

        deps
    }

    /// Returns `true` if the `image` depends on the `dependency` or the dependency is in common
    /// dependencies.
    pub fn depends_on(&self, image: &str, dependency: &str) -> bool {
        if let Some(common_deps) = self.inner.get(COMMON_DEPS_KEY) {
            if common_deps.contains(dependency) {
                return true;
            }
        }
        if let Some(image_deps) = self.inner.get(image) {
            return image_deps.contains(dependency);
        }

        false
    }

    pub fn inner(&self) -> &DepsMap {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut DepsMap {
        &mut self.inner
    }

    /// Updates the dependencies of the given `image` by extending them or inserting the new ones
    /// if the entry doesn't yet exist.
    pub fn update_or_insert<I, V, D>(&mut self, image: I, deps: D)
    where
        I: Into<String>,
        V: Into<String>,
        D: IntoIterator<Item = V>,
    {
        let image = image.into();
        if let Some(image_deps) = self.inner.get_mut(&image) {
            image_deps.extend(deps.into_iter().map(|s| s.into()));
        } else {
            self.inner
                .insert(image, deps.into_iter().map(|s| s.into()).collect());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    macro_rules! test_deps {
    (
        input = $inp:expr,
        want = $(
            $image:ident => $($dep:literal),+
        );+) => {
            let input: YamlValue = serde_yaml::from_str($inp).unwrap();
            let input = input.as_mapping().unwrap().get(&serde_yaml::Value::String("build_depends".to_string())).unwrap().clone();
            let got = Dependencies::try_from(input).unwrap();

            $(
            let mut $image = HashSet::new();
                $(
            $image.insert($dep);
                )+

            assert_eq!($image, got.resolve_names(stringify!($image)));
            )+

        }
}

    #[test]
    fn parses_deps() {
        test_deps!(
        input = r#"
build_depends:
  all: ["gcc", "pkg-config", "git"]
  centos8: ["cargo", "openssl-devel"]
  debian10: ["curl", "libssl-dev"]
"#,
        want =
            all      => "gcc", "pkg-config", "git";
            centos8  => "cargo", "openssl-devel", "gcc", "pkg-config", "git";
            debian10 => "curl", "libssl-dev", "gcc", "pkg-config", "git"
        );
        test_deps!(
        input = r#"
build_depends:
  - gcc
  - pkg-config
  - git
"#,
        want =
            all      => "gcc", "pkg-config", "git"
        );
    }

    #[test]
    fn parses_joined_deps() {
        test_deps!(
        input = r#"
build_depends:
  centos8+fedora34: [ cargo,  openssl-devel ]
  debian10+ubuntu20: [ libssl-dev ]
  debian10: [ curl ]
"#,
        want =
            centos8 => "cargo", "openssl-devel";
            fedora34 => "cargo", "openssl-devel";
            debian10 => "curl", "libssl-dev";
            ubuntu20 => "libssl-dev"
        );
    }
}
