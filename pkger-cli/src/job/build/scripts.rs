use crate::job::build::BuildContainerCtx;
use crate::{Error, Result};
use pkger_core::container::ExecOpts;

use std::path::PathBuf;
use tracing::{debug, info, info_span, trace, Instrument};

macro_rules! run_script {
    ($phase:literal, $script:expr, $dir:expr,  $ctx:ident) => {{
        let _span = info_span!($phase);
        async move {
            trace!(script = ?$script);
            info!(concat!("executing ", $phase, " scripts"));
            let mut opts = ExecOpts::default();
            let mut _dir;

            if let Some(dir) = &$script.working_dir {
                trace!(working_dir = %dir.display());
                let mut dir_s = dir.to_string_lossy().to_string();
                let bld_dir = $ctx.container_bld_dir.to_string_lossy();
                let out_dir = $ctx.container_out_dir.to_string_lossy();
                dir_s = dir_s.replace("$PKGER_BLD_DIR", bld_dir.as_ref());
                dir_s = dir_s.replace("$PKGER_OUT_DIR", out_dir.as_ref());
                _dir = PathBuf::from(dir_s);
                opts = opts.working_dir(_dir.as_path());
            } else {
                trace!(working_dir = %$dir.display(), "using default");
                opts = opts.working_dir($dir);
            }

            if let Some(shell) = &$script.shell {
                trace!(shell = %shell);
                opts = opts.shell(shell.as_str());
            }

            for cmd in &$script.steps {
                if let Some(images) = &cmd.images {
                    trace!(images = ?images, "only execute on");
                    if !images.contains(&$ctx.image.name) {
                        trace!(image = %$ctx.image.name, "not found in images");
                        if !cmd.has_target_specified() {
                            debug!(command = %cmd.cmd, "skipping, excluded by image filter");
                            continue;
                        }
                    }
                }

                if !cmd.should_run_on($ctx.target.build_target()) {
                    debug!(command = %cmd.cmd, "skipping, shouldn't run on target");
                    continue;
                }

                debug!(command = %cmd.cmd, "running");
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
