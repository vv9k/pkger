#![allow(dead_code)]
use crate::Result;

use anyhow::Context as ErrContext;
use serde::Deserialize;
use serde_yaml::{Mapping, Sequence, Value as YamlValue};
use std::collections::HashMap;
use std::convert::TryFrom;

pub static COMMON_PATCHES_KEY: &str = "all";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct Patch {
    patch: String,
    #[serde(default)]
    strip: u8,
    images: Option<Vec<String>>,
}

impl Patch {
    pub fn new<S: Into<String>, I: IntoIterator<Item = S>>(
        patch: S,
        strip: u8,
        images: Option<I>,
    ) -> Self {
        Self {
            patch: patch.into(),
            strip,
            images: images.map(|images| images.into_iter().map(|s| s.into()).collect()),
        }
    }

    pub fn images(&self) -> Option<&[String]> {
        self.images.as_deref()
    }

    pub fn patch(&self) -> &str {
        &self.patch
    }

    pub fn strip_level(&self) -> u8 {
        self.strip
    }
}

impl TryFrom<YamlValue> for Patch {
    type Error = crate::Error;

    fn try_from(value: YamlValue) -> Result<Self> {
        Self::try_from(&value)
    }
}

impl TryFrom<&YamlValue> for Patch {
    type Error = crate::Error;

    fn try_from(value: &YamlValue) -> Result<Self> {
        if let YamlValue::String(patch) = value {
            Ok(Patch::new(patch, 0, None::<Vec<_>>))
        } else if let YamlValue::Mapping(_) = value {
            serde_yaml::from_value(value.clone()).context("deserializing patch")
        } else {
            Err(anyhow!(
                "expected a string or a mapping as patch, found `{:?}`",
                value
            ))
        }
    }
}

type PatchesMap = HashMap<String, Vec<Patch>>;

#[derive(Clone, Debug, PartialEq)]
pub struct Patches {
    inner: PatchesMap,
}

impl Default for Patches {
    fn default() -> Self {
        let mut patches = Self {
            inner: HashMap::new(),
        };

        // ensure the COMMON_patches_KEY entry is created by default
        patches
            .inner
            .insert(COMMON_PATCHES_KEY.to_string(), Vec::new());
        patches
    }
}

impl TryFrom<Mapping> for Patch {
    type Error = crate::Error;

    fn try_from(mapping: Mapping) -> Result<Self, Self::Error> {
        Self::try_from(&mapping)
    }
}

impl TryFrom<&Mapping> for Patch {
    type Error = crate::Error;

    fn try_from(mapping: &Mapping) -> Result<Self, Self::Error> {
        let name = mapping.get(&YamlValue::from("patch"));
        if name.is_none() {
            return Err(anyhow!("missing `patch` field"));
        }

        let name = name.unwrap();
        if !name.is_string() {
            return Err(anyhow!(
                "expected a string as patch name, found `{:?}`",
                name
            ));
        }
        let name = name.as_str().unwrap().to_string();
        let level = mapping
            .get(&YamlValue::from("strip"))
            .cloned()
            .unwrap_or_else(|| YamlValue::from(0));

        if !level.is_number() {
            return Err(anyhow!(
                "expected a number as strip level for patch, found `{:?}`",
                level
            ));
        }

        match u8::try_from(level.as_u64().unwrap()) {
            Ok(level) => Ok(Patch::new(name, level, None::<Vec<_>>)),
            Err(_) => Err(anyhow!(
                "expected a number in range of 0-255, found `{:?}`",
                level
            )),
        }
    }
}

impl TryFrom<Mapping> for Patches {
    type Error = crate::Error;

    fn try_from(table: Mapping) -> Result<Self, Self::Error> {
        let mut patches = Self::default();
        for (image, image_patches) in table {
            if image_patches.is_sequence() {
                let mut patches_vec = Vec::new();
                for patch in image_patches.as_sequence().unwrap() {
                    patches_vec.push(Patch::try_from(patch)?);
                }
                patches.inner_mut().insert(
                    image
                        .as_str()
                        .map(|s| s.to_string())
                        .ok_or_else(|| anyhow!("expected image name"))?,
                    patches_vec,
                );
            } else {
                return Err(anyhow!(
                    "expected array of patches, found `{:?}`",
                    image_patches
                ));
            }
        }
        Ok(patches)
    }
}

impl TryFrom<Sequence> for Patches {
    type Error = crate::Error;
    fn try_from(array: Sequence) -> Result<Self> {
        let mut patches = Self::default();
        let mut patches_vec = Vec::new();
        for patch in array {
            patches_vec.push(Patch::try_from(patch)?);
        }
        patches
            .inner_mut()
            .insert(COMMON_PATCHES_KEY.to_string(), patches_vec);

        Ok(patches)
    }
}

impl TryFrom<YamlValue> for Patches {
    type Error = crate::Error;
    fn try_from(patches: YamlValue) -> Result<Self> {
        match patches {
            YamlValue::Mapping(table) => Self::try_from(table),
            YamlValue::Sequence(array) => Self::try_from(array),
            _ => Err(anyhow!(
                "expected a map or array of patches, found `{:?}`",
                patches
            )),
        }
    }
}

impl Patches {
    pub fn resolve_names(&self, image: &str) -> Vec<&Patch> {
        // it's ok to unwrap here, the new function adds an empty vec on initialization
        let mut patches = Vec::new();
        if let Some(common_patches) = self.inner.get(COMMON_PATCHES_KEY) {
            common_patches.iter().for_each(|p| {
                patches.push(p);
            });
        }
        if image != COMMON_PATCHES_KEY {
            if let Some(image_patches) = self.inner.get(image) {
                image_patches.iter().for_each(|p| {
                    patches.push(p);
                });
            }
        }

        patches
    }

    pub fn inner(&self) -> &PatchesMap {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut PatchesMap {
        &mut self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    macro_rules! test_patches {
    (
        input = $inp:expr,
        want = $(
            $image:ident => $($patch:literal $level:tt),+
        );+) => {
            let input: YamlValue = serde_yaml::from_str($inp).unwrap();
            dbg!(&input);
            let input = input.as_mapping().unwrap().get(&serde_yaml::Value::String("patches".to_string())).unwrap().clone();
            let got = Patches::try_from(input).unwrap();

            $(
            let mut names = got.resolve_names(stringify!($image));
            let mut $image = vec![
                $(
                    Patch::new($patch, $level)
                ),+
            ];

            names.sort();
            $image.sort();

            assert_eq!($image.len(), names.len());

            for (got, want) in names.iter().zip($image.iter())  {
                assert_eq!(*got, want);
            }

            )+

        }
}

    #[test]
    fn parses_patches() {
        test_patches!(
        input = r#"
patches:
  all: ["test.patch", "1.patch", "http://remote.com/file.patch"]
  centos8:
    - patch: only-cent.patch
      strip: 1
  debian10:
    - patch: only-deb.patch
      strip: 2
"#,
        want =
            all      => "test.patch" 0, "1.patch" 0, "http://remote.com/file.patch" 0;
            centos8  => "test.patch" 0, "1.patch" 0, "http://remote.com/file.patch" 0, "only-cent.patch" 1;
            debian10 => "test.patch" 0, "1.patch" 0, "http://remote.com/file.patch" 0, "only-deb.patch" 2
        );
    }
}
