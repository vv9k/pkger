use crate::recipe::BuildTarget;
use crate::{Error, Result};

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value as YamlValue};
use std::convert::TryFrom;

#[derive(Clone, Deserialize, Serialize, Debug, Eq, PartialEq, Hash)]
pub struct ImageTarget {
    pub image: String,
    pub target: BuildTarget,
}

impl ImageTarget {
    pub fn new<I: Into<String>>(image: I, target: &BuildTarget) -> Self {
        Self {
            image: image.into(),
            target: target.clone(),
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

            Ok(ImageTarget { image, target })
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
                target: BuildTarget::default(),
            }),
            value => Err(anyhow!(
                "expected a map or string for image, found `{:?}`",
                value
            )),
        }
    }
}
