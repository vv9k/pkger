use crate::image::ImageState;
use crate::job::build::BuildContainerCtx;
use crate::recipe::{BuildTarget, Recipe};
use crate::Result;

use std::collections::HashSet;
use tracing::{info, info_span, trace, Instrument};

impl<'job> BuildContainerCtx<'job> {
    pub fn recipe_deps(&self, state: &ImageState) -> HashSet<String> {
        if let Some(deps) = &self.recipe.metadata.build_depends {
            deps.resolve_names(&state.image)
        } else {
            HashSet::new()
        }
    }

    #[allow(dead_code)]
    pub async fn install_recipe_deps(&self, state: &ImageState) -> Result<()> {
        let span = info_span!("recipe-deps");
        let deps = self.recipe_deps(&state).into_iter().collect::<Vec<_>>();

        async move { self._install_deps(&deps, &state).await }
            .instrument(span)
            .await
    }

    #[allow(dead_code)]
    pub async fn install_pkger_deps(&self, state: &ImageState) -> Result<()> {
        let span = info_span!("default-deps");
        async move {
            self._install_deps(
                &pkger_deps(&self.target, &self.recipe)
                    .into_iter()
                    .collect::<Vec<_>>()[..],
                &state,
            )
            .await
        }
        .instrument(span)
        .await
    }

    async fn _install_deps(&self, deps: &[String], state: &ImageState) -> Result<()> {
        let span = info_span!("install-deps");
        async move {
            info!("installing dependencies");
            let pkg_mngr = state.os.package_manager();
            let pkg_mngr_name = pkg_mngr.as_ref();

            if deps.is_empty() {
                trace!("no dependencies to install");
                return Ok(());
            }

            trace!(deps = ?deps, "resolved dependency names");
            let deps = deps.join(" ");

            if pkg_mngr_name.starts_with("apt") {
                self.checked_exec(
                    &[pkg_mngr_name, &pkg_mngr.update_repos_args().join(" ")].join(" "),
                    None,
                    None,
                )
                .await?;
            }

            let cmd = [pkg_mngr.as_ref(), &pkg_mngr.install_args().join(" "), &deps].join(" ");
            trace!(command = %cmd, "installing with");

            self.checked_exec(&cmd, None, None).await.map(|_| ())
        }
        .instrument(span)
        .await
    }
}

pub fn pkger_deps(target: &BuildTarget, recipe: &Recipe) -> HashSet<String> {
    let mut deps = HashSet::new();
    deps.insert("tar".to_string());
    match target {
        BuildTarget::Rpm => {
            deps.insert("rpm-build".to_string());
        }
        BuildTarget::Deb => {
            deps.insert("dpkg".to_string());
        }
        BuildTarget::Gzip => {
            deps.insert("gzip".to_string());
        }
    }
    if recipe.metadata.git.is_some() {
        deps.insert("git".to_string());
    } else if let Some(src) = &recipe.metadata.source {
        if src.starts_with("http") {
            deps.insert("curl".to_string());
        }
        if src.ends_with(".zip") {
            deps.insert("zip".to_string());
        }
    }

    deps
}
