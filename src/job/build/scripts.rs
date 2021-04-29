use crate::container::ExecOpts;
use crate::job::build::BuildContainerCtx;
use crate::{Error, Result};

use tracing::{info, info_span, trace, Instrument};

macro_rules! run_script {
    ($phase:literal, $script:expr, $dir:expr,  $ctx:ident) => {{
        let _span = info_span!($phase);
        async move {
            trace!(script = ?$script);
            info!(concat!("executing ", $phase, " scripts"));
            let mut opts = ExecOpts::default();

            if let Some(dir) = &$script.working_dir {
                trace!(working_dir = %dir.display());
                opts = opts.working_dir(dir.as_path());
            } else {
                opts = opts.working_dir($dir);
            }

            if let Some(shell) = &$script.shell {
                trace!(shell = %shell);
                opts = opts.shell(shell.as_str());
            }

            for cmd in &$script.steps {
                if !cmd.images.is_empty() {
                    trace!(images = ?cmd.images, "only execute on");
                    if !cmd.images.contains(&$ctx.image.name) {
                        trace!(image = %$ctx.image.name, "not found in images");
                        if !cmd.has_target_specified() {
                            trace!("skipping");
                            continue;
                        }
                    }
                }

                if !cmd.should_run_on(&$ctx.target) {
                    trace!(command = %cmd.cmd, "skipping, shouldn't run on target");
                    continue;
                }

                $ctx.checked_exec(&opts.clone().cmd(&cmd.cmd).build())
                    .await?;
            }

            Ok::<_, Error>(())
        }
        .instrument(_span)
        .await?;
    }};
}
impl<'job> BuildContainerCtx<'job> {
    pub async fn execute_scripts(&self) -> Result<()> {
        let span = info_span!("exec-scripts");
        async move {
            if let Some(config_script) = &self.recipe.configure_script {
                run_script!("configure", config_script, self.container_bld_dir, self);
            } else {
                info!("no configure steps to run");
            }

            let build_script = &self.recipe.build_script;
            run_script!("build", build_script, self.container_bld_dir, self);

            if let Some(install_script) = &self.recipe.install_script {
                run_script!("install", install_script, self.container_out_dir, self);
            } else {
                info!("no install steps to run");
            }

            Ok(())
        }
        .instrument(span)
        .await
    }
}
