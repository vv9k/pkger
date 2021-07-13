use crate::build::container::Context;
use crate::image::ImageState;
use crate::recipe::{BuildTarget, Recipe};

use std::collections::HashSet;

pub fn recipe_deps<'ctx>(ctx: &Context<'ctx>, state: &ImageState) -> HashSet<&'ctx str> {
    if let Some(deps) = &ctx.build_ctx.recipe.metadata.build_depends {
        deps.resolve_names(&state.image)
    } else {
        HashSet::new()
    }
}

pub fn pkger_deps(target: &BuildTarget, recipe: &Recipe) -> HashSet<&'static str> {
    let mut deps = HashSet::new();
    deps.insert("tar");
    match target {
        BuildTarget::Rpm => {
            deps.insert("rpm-build");
            deps.insert("util-linux"); // for setarch
        }
        BuildTarget::Deb => {
            deps.insert("dpkg");
        }
        BuildTarget::Gzip => {
            deps.insert("gzip");
        }
        BuildTarget::Pkg => {
            deps.insert("base-devel");
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

    if recipe.metadata.patches.is_some() {
        deps.insert("patch");
    }

    deps
}
