use crate::job::build::BuildContainerCtx;
use crate::recipe::{BuildScript, ConfigureScript, InstallScript};
use crate::Result;

use tracing::{info, info_span, trace, Instrument};

impl<'job> BuildContainerCtx<'job> {
    pub async fn execute_scripts(&self) -> Result<()> {
        let span = info_span!("exec-scripts");
        async move {
            if let Some(config_script) = &self.recipe.configure_script {
                self.run_configure(&config_script).await?;
            } else {
                info!("no configure steps to run");
            }

            self.run_build(&self.recipe.build_script).await?;

            if let Some(install_script) = &self.recipe.install_script {
                self.run_install(&install_script).await?;
            } else {
                info!("no install steps to run");
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    async fn run_configure(&self, config_script: &ConfigureScript) -> Result<()> {
        let span = info_span!("configure");
        async move {
            trace!(script = ?config_script);
            info!("executing configure scripts");
            let working_dir = if let Some(dir) = &config_script.working_dir {
                trace!(working_dir = %dir.display());
                Some(dir.as_path())
            } else {
                None
            };
            let shell = if let Some(shell) = &config_script.shell {
                trace!(shell = %shell);
                Some(shell.as_str())
            } else {
                None
            };
            for cmd in &config_script.steps {
                if !cmd.images.is_empty() && !self.simple {
                    trace!(images = ?cmd.images, "only execute on");
                    if !cmd.images.contains(&self.image.name) {
                        trace!(image = %self.image.name, "not found, skipping");
                        continue;
                    }
                }

                if !cmd.should_run_on(&self.target) {
                    trace!(command = %cmd.cmd, "skipping, shouldn't run on target");
                    continue;
                }

                self.checked_exec(&cmd.cmd, working_dir, shell, None)
                    .await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    async fn run_build(&self, build_script: &BuildScript) -> Result<()> {
        let span = info_span!("build");
        async move {
            trace!(script = ?build_script);
            info!("executing build scripts");
            let working_dir = if let Some(dir) = &build_script.working_dir {
                trace!(working_dir = %dir.display());
                Some(dir.as_path())
            } else {
                Some(self.container_bld_dir)
            };
            let shell = if let Some(shell) = &build_script.shell {
                trace!(shell = %shell);
                Some(shell.as_str())
            } else {
                None
            };
            for cmd in &build_script.steps {
                if !cmd.images.is_empty() && !self.simple {
                    trace!(images = ?cmd.images, "only execute on");
                    if !cmd.images.contains(&self.image.name) {
                        trace!(image = %self.image.name, "not found, skipping");
                        continue;
                    }
                }

                if !cmd.should_run_on(&self.target) {
                    trace!(command = %cmd.cmd, "skipping, shouldn't run on target");
                    continue;
                }
                self.checked_exec(&cmd.cmd, working_dir, shell, None)
                    .await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    async fn run_install(&self, install_script: &InstallScript) -> Result<()> {
        let span = info_span!("install");
        async move {
            trace!(script = ?install_script);
            info!("executing install scripts");
            let working_dir = if let Some(dir) = &install_script.working_dir {
                trace!(working_dir = %dir.display());
                Some(dir.as_path())
            } else {
                Some(self.container_out_dir)
            };
            let shell = if let Some(shell) = &install_script.shell {
                trace!(shell = %shell);
                Some(shell.as_str())
            } else {
                None
            };
            for cmd in &install_script.steps {
                if !cmd.images.is_empty() && !self.simple {
                    trace!(images = ?cmd.images, "only execute on");
                    if !cmd.images.contains(&self.image.name) {
                        trace!(image = %self.image.name, "not found, skipping");
                        continue;
                    }
                }

                if !cmd.should_run_on(&self.target) {
                    trace!(command = %cmd.cmd, "skipping, shouldn't run on target");
                    continue;
                }

                self.checked_exec(&cmd.cmd, working_dir, shell, None)
                    .await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }
}
