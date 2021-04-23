#![allow(dead_code)]
use crate::Result;

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
        deps.inner
            .insert(COMMON_DEPS_KEY.to_string(), HashSet::new());
        deps
    }
}

impl TryFrom<toml::Value> for Dependencies {
    type Error = crate::Error;
    fn try_from(deps: toml::Value) -> Result<Self> {
        if let toml::Value::Table(table) = deps {
            let mut deps = Self::default();
            for (image, image_deps) in table {
                if image_deps.is_array() {
                    deps.inner_mut().insert(
                        image.to_string(),
                        image_deps
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.to_string().trim_matches('"').to_string())
                            .collect::<HashSet<_>>(),
                    );
                } else {
                    return Err(anyhow!(
                        "expected array of dependencies, found `{:?}`",
                        image_deps
                    ));
                }
            }
            Ok(deps)
        } else if let toml::Value::Array(array) = deps {
            let mut deps = Self::default();
            deps.inner_mut().insert(
                COMMON_DEPS_KEY.to_string(),
                array.iter().fold(HashSet::new(), |mut set, it| {
                    set.insert(it.to_string());
                    set
                }),
            );

            Ok(deps)
        } else {
            Err(anyhow!(
                "expected a map or array of dependencies, found `{:?}`",
                deps
            ))
        }
    }
}

impl Dependencies {
    pub fn resolve_names(&self, image: &str) -> HashSet<String> {
        // it's ok to unwrap here, the new function adds an empty hashset on initialization
        let mut deps = self.inner.get(COMMON_DEPS_KEY).unwrap().clone();
        if let Some(image_deps) = self.inner.get(image) {
            image_deps.iter().for_each(|dep| {
                deps.insert(dep.clone());
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
            let input: toml::Value = toml::from_str($inp).unwrap();
            dbg!(&input);
            let input = input.as_table().unwrap().get("build_depends").unwrap().clone();
            let got = Dependencies::try_from(input).unwrap();

            $(
            let mut $image = HashSet::new();
                $(
            $image.insert($dep.to_string());
                )+

            assert_eq!($image, got.resolve_names(stringify!($image)));
            )+

        }
}

    #[test]
    fn parses_deps() {
        test_deps!(
        input = r#"
[build_depends]
all = ["gcc", "pkg-config", "git"]
centos8 = ["cargo", "openssl-devel"]
debian10 = ["curl", "libssl-dev"]
"#,
        want =
            all      => "gcc", "pkg-config", "git";
            centos8  => "cargo", "openssl-devel", "gcc", "pkg-config", "git";
            debian10 => "curl", "libssl-dev", "gcc", "pkg-config", "git"
        );
    }
}
