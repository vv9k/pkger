use crate::build::container::Context;
use crate::image::{Image, ImageState};
use crate::recipe::{BuildTarget, Recipe};

use std::collections::HashSet;

pub fn recipe<'ctx>(ctx: &Context<'ctx>, state: &ImageState) -> HashSet<&'ctx str> {
    if let Some(deps) = &ctx.build.recipe.metadata.build_depends {
        let mut _deps = deps.resolve_names(&state.image);
        let simple = Image::simple(*ctx.build.target.build_target()).image;
        if state.image != simple {
            _deps.extend(deps.resolve_names(simple));
        }

        return _deps;
    }
    HashSet::new()
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

    let mut is_http = false;
    let mut is_zip = false;

    for src in &recipe.metadata.source {
        if src.starts_with("http") {
            is_http = true;
        }
        if src.ends_with(".zip") {
            is_zip = true;
        }
    }
    if is_http {
        deps.insert("curl");
    }
    if is_zip {
        deps.insert("zip");
    }

    if recipe.metadata.patches.is_some() {
        deps.insert("patch");
    }

    deps
}
