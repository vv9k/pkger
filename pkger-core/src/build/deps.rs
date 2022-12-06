use crate::image::Image;
use crate::recipe::{BuildTarget, Dependencies, Recipe};

use std::collections::HashSet;

pub fn recipe_and_default<'ctx>(
    deps: Option<&'ctx Dependencies>,
    recipe_: &Recipe,
    build_target: BuildTarget,
    state_image: &str,
    enable_gpg: bool,
) -> HashSet<&'ctx str> {
    let mut deps_out = default(&build_target, recipe_, enable_gpg);
    let recipe = recipe(deps, build_target, state_image);
    deps_out.extend(recipe);
    deps_out
}

pub fn recipe<'ctx>(
    deps: Option<&'ctx Dependencies>,
    build_target: BuildTarget,
    state_image: &str,
) -> HashSet<&'ctx str> {
    let mut deps_out = HashSet::new();
    if let Some(deps) = &deps {
        deps_out.extend(deps.resolve_names(state_image));
        let simple = Image::simple(build_target).name;
        deps_out.extend(deps.resolve_names(simple));
    }
    deps_out
}

fn default(target: &BuildTarget, recipe: &Recipe, enable_gpg: bool) -> HashSet<&'static str> {
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
