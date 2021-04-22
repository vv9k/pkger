use crate::image::ImageState;
use crate::job::build::BuildContainerCtx;
use crate::recipe::BuildTarget;
use crate::Result;

use tracing::{info, info_span, trace, Instrument};

impl<'job> BuildContainerCtx<'job> {
    pub async fn install_recipe_deps(&self, state: &ImageState) -> Result<()> {
        let span = info_span!("recipe-deps");
        async move {
            let deps = if let Some(deps) = &self.recipe.metadata.build_depends {
                deps.resolve_names(&state.image)
            } else {
                vec![]
            };

            self._install_deps(&deps, &state).await
        }
        .instrument(span)
        .await
    }

    pub async fn install_pkger_deps(&self, state: &ImageState) -> Result<()> {
        let span = info_span!("default-deps");
        async move {
            let mut deps = vec!["tar"];
            match self.target {
                BuildTarget::Rpm => {
                    deps.push("rpm-build");
                }
                BuildTarget::Deb => {
                    deps.push("dpkg");
                }
                BuildTarget::Gzip => {
                    deps.push("gzip");
                }
            }
            if self.recipe.metadata.git.is_some() {
                deps.push("git");
            } else if let Some(src) = &self.recipe.metadata.source {
                if src.starts_with("http") {
                    deps.push("curl");
                }
                if src.ends_with(".zip") {
                    deps.push("zip");
                }
            }

            let deps = deps.into_iter().map(str::to_string).collect::<Vec<_>>();

            self._install_deps(&deps, &state).await
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
