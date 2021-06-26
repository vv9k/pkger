use crate::recipe::{BuildTarget, Os};
use crate::{Error, Result};

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value as YamlValue};
use std::convert::TryFrom;

#[derive(Clone, Deserialize, Serialize, Debug, Eq, PartialEq, Hash)]
pub struct ImageTarget {
    pub image: String,
    pub build_target: BuildTarget,
    pub os: Option<Os>,
}

impl ImageTarget {
    pub fn new<I, O>(image: I, build_target: BuildTarget, os: Option<O>) -> Self
    where
        I: Into<String>,
        O: AsRef<str>,
    {
        Self {
            image: image.into(),
            build_target,
            os: os.map(|os| Os::new(os, None::<&str>).unwrap()),
        }
    }
}

impl TryFrom<Mapping> for ImageTarget {
    type Error = Error;

    fn try_from(map: Mapping) -> Result<Self> {
        if let Some(image) = map.get(&YamlValue::from("name")) {
            if !image.is_string() {
                return Err(anyhow!(
                    "expected a string as image name, found `{:?}`",
                    image
                ));
            }
            let image = image.as_str().unwrap().to_string();

            let target = if let Some(target) = map.get(&YamlValue::from("target")) {
                if !target.is_string() {
                    return Err(anyhow!(
                        "expected a string as image target, found `{:?}`",
                        image
                    ));
                } else {
                    BuildTarget::try_from(target.as_str().unwrap())?
                }
            } else {
                BuildTarget::default()
            };

            let os = if let Some(os) = map.get(&YamlValue::from("os")) {
                if !os.is_string() {
                    return Err(anyhow!(
                        "expected a string as image os, found `{:?}`",
                        image
                    ));
                } else {
                    Some(Os::new(os.as_str().unwrap(), None::<&str>)?)
                }
            } else {
                None
            };

            Ok(ImageTarget {
                image,
                build_target: target,
                os,
            })
        } else {
            Err(anyhow!("image name not found in `{:?}`", map))
        }
    }
}

impl TryFrom<YamlValue> for ImageTarget {
    type Error = Error;
    fn try_from(value: YamlValue) -> Result<Self> {
        match value {
            YamlValue::Mapping(map) => Self::try_from(map),
            YamlValue::String(image) => Ok(Self {
                image,
                build_target: BuildTarget::default(),
                os: None,
            }),
            value => Err(anyhow!(
                "expected a map or string for image, found `{:?}`",
                value
            )),
        }
    }
}
