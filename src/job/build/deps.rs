use crate::image::ImageState;
use crate::job::build::BuildContainerCtx;
use crate::recipe::{BuildTarget, Recipe};

use std::collections::HashSet;

impl<'job> BuildContainerCtx<'job> {
    pub fn recipe_deps(&self, state: &ImageState) -> HashSet<&str> {
        if let Some(deps) = &self.recipe.metadata.build_depends {
            deps.resolve_names(&state.image)
        } else {
            HashSet::new()
        }
    }
}

pub fn pkger_deps(target: &BuildTarget, recipe: &Recipe) -> HashSet<&'static str> {
    let mut deps = HashSet::new();
    deps.insert("tar");
    match target {
        BuildTarget::Rpm => {
            deps.insert("rpm-build");
        }
        BuildTarget::Deb => {
            deps.insert("dpkg");
        }
        BuildTarget::Gzip => {
            deps.insert("gzip");
        }
    }
    if recipe.metadata.git.is_some() {
        deps.insert("git");
    } else if let Some(src) = &recipe.metadata.source {
        if src.starts_with("http") {
            deps.insert("curl");
        }
        if src.ends_with(".zip") {
            deps.insert("zip");
        }
    }

    deps
}
