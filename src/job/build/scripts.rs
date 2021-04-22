use crate::job::build::BuildContainerCtx;
use crate::Result;

use tracing::{info, info_span, trace, Instrument};

impl<'job> BuildContainerCtx<'job> {
    pub async fn execute_scripts(&self) -> Result<()> {
        let span = info_span!("exec-scripts");
        async move {
            if let Some(config_script) = &self.recipe.configure_script {
                let script_span = info_span!("configure");
                info!(parent: &script_span, "executing configure scripts");
                let working_dir = if let Some(dir) = &config_script.working_dir {
                    Some(dir.as_path())
                } else {
                    None
                };
                let shell = if let Some(shell) = &config_script.shell {
                    Some(shell.as_str())
                } else {
                    None
                };
                for cmd in &config_script.steps {
                    if !cmd.images.is_empty() {
                        trace!(parent: &script_span, images = ?cmd.images, "only execute on");
                        if !cmd.images.contains(&self.image.name) {
                            trace!(parent: &script_span, image = %self.image.name, "not found, skipping");
                            continue;
                        }
                    }
                    let out = self.checked_exec(&cmd.cmd, working_dir, shell).instrument(script_span.clone()).await?;

                    if out.exit_code != 0 {
                        return Err(anyhow!(
                            "command `{}` failed with exit code {}\nError:\n{}",
                            &cmd.cmd,
                            out.exit_code,
                            out.stderr.join("\n")
                        ));
                    }
                }
            } else {
                info!("no configure steps to run");
            }

            let script_span = info_span!("build");
            let working_dir = if let Some(dir) = &self.recipe.build_script.working_dir {
                Some(dir.as_path())
            } else {
                Some(self.container_bld_dir)
            };
            let shell = if let Some(shell) = &self.recipe.build_script.shell {
                Some(shell.as_str())
            } else {
                None
            };
            info!(parent: &script_span, "executing build scripts");
            for cmd in &self.recipe.build_script.steps {
                if !cmd.images.is_empty() {
                    trace!(parent: &script_span, images = ?cmd.images, "only execute on");
                    if !cmd.images.contains(&self.image.name) {
                        trace!(parent: &script_span, image = %self.image.name, "not found, skipping");
                        continue;
                    }
                }

                self.checked_exec(&cmd.cmd, working_dir, shell).instrument(script_span.clone()).await?;
            }

            if let Some(install_script) = &self.recipe.install_script {
                let script_span = info_span!("install");
                let working_dir = if let Some(dir) = &install_script.working_dir {
                    Some(dir.as_path())
                } else {
                    Some(self.container_out_dir)
                };
                let shell = if let Some(shell) = &install_script.shell {
                    Some(shell.as_str())
                } else {
                    None
                };
                info!(parent: &script_span, "executing install scripts");
                for cmd in &install_script.steps {
                    if !cmd.images.is_empty() {
                        trace!(parent: &script_span, images = ?cmd.images, "only execute on");
                        if !cmd.images.contains(&self.image.name) {
                            trace!(parent: &script_span, image = %self.image.name, "not found, skipping");
                            continue;
                        }
                    }

                    self.checked_exec(&cmd.cmd, working_dir, shell).instrument(script_span.clone()).await?;
                }
            } else {
                info!("no install steps to run");
            }

            Ok(())
        }
        .instrument(span)
        .await
    }
}
