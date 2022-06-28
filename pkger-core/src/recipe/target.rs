use crate::recipe::metadata::{BuildTarget, ImageTarget, Os};

use serde::{Deserialize, Serialize};

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
