use crate::build::container::Context;
use crate::image::ImageState;
use crate::recipe::{BuildTarget, Recipe};

use std::collections::HashSet;

pub fn recipe<'ctx>(ctx: &Context<'ctx>, state: &ImageState) -> HashSet<&'ctx str> {
    if let Some(deps) = &ctx.build.recipe.metadata.build_depends {
        deps.resolve_names(&state.image)
    } else {
        HashSet::new()
    }
}

pub fn default(target: &BuildTarget, recipe: &Recipe, enable_gpg: bool) -> HashSet<&'static str> {
    let mut deps = HashSet::new();
    deps.insert("tar");
    match target {
        BuildTarget::Rpm => {
            deps.insert("rpm-build");
            deps.insert("util-linux"); // for setarch

            if enable_gpg {
                deps.insert("gnupg2");
                deps.insert("rpm-sign");
            }
        }
        BuildTarget::Deb => {
            deps.insert("dpkg");

            if enable_gpg {
                deps.insert("gnupg2");
                deps.insert("dpkg-sig");
            }
        }
        BuildTarget::Gzip => {
            deps.insert("gzip");
        }
        BuildTarget::Pkg => {
            deps.insert("base-devel");
        }
        BuildTarget::Apk => {
            deps.insert("alpine-sdk");
            deps.insert("sudo");
            deps.insert("bash");
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
