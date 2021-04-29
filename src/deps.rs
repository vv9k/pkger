#![allow(dead_code)]
use crate::Result;

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
                deps.inner_mut().insert(
                    image
                        .as_str()
                        .map(|s| s.to_string())
                        .ok_or_else(|| anyhow!("expected image name"))?,
                    deps_set,
                );
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
    pub fn resolve_names(&self, image: &str) -> HashSet<&str> {
        // it's ok to unwrap here, the new function adds an empty hashset on initialization
        let mut deps = HashSet::new();
        if let Some(common_deps) = self.inner.get(COMMON_DEPS_KEY) {
            common_deps.iter().for_each(|dep| {
                deps.insert(dep.as_str());
            });
        }
        if let Some(image_deps) = self.inner.get(image) {
            image_deps.iter().for_each(|dep| {
                deps.insert(dep.as_str());
            });
        }

        deps
    }

    pub fn inner(&self) -> &DepsMap {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut DepsMap {
        &mut self.inner
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
            dbg!(&input);
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
    }
}
