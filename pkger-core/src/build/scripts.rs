use crate::build::container::Context;
use crate::log::{debug, info, trace, BoxedCollector};
use crate::runtime::container::ExecOpts;
use crate::template;
use crate::{Error, Result};

use std::path::PathBuf;

macro_rules! run_script {
    ($phase:literal, $script:expr, $dir:expr,  $ctx:ident, $logger:ident) => {{
        info!($logger => "running script for {} phase", $phase);
        trace!($logger => "{:?}", $script);
        info!($logger => concat!("executing ", $phase, " scripts"));
        let mut opts = ExecOpts::default();
        let mut _dir;

        if let Some(dir) = &$script.working_dir {
            _dir = PathBuf::from(template::render(dir.to_string_lossy(), $ctx.vars.inner()));
            trace!($logger => "Working directory: {}", _dir.display());
            opts = opts.working_dir(&_dir);
        } else {
            trace!($logger => "Working directory: {} (Default)", $dir.display());
            opts = opts.working_dir($dir);
        }

        if let Some(shell) = &$script.shell {
            trace!($logger => "Shell: {}", shell);
            opts = opts.shell(shell.as_str());
        }

        for cmd in &$script.steps {
            debug!($logger => "Processing: {:?}", cmd);
            if let Some(images) = &cmd.images {
                trace!($logger => "only execute on {:?}", images);
                if !images.contains(&$ctx.build.target.image().to_owned()) {
                    trace!($logger => "'{}' not found in images", $ctx.build.target.image());
                    if !cmd.has_target_specified() {
                        debug!($logger => "skipping command, excluded by image filter");
                        continue;
                    }
                }
            }

            let target = $ctx.build.target.build_target();
            if !cmd.should_run_on_target(target) {
                trace!($logger => "skipping command, shouldn't run on target {:?}", target);
                continue;
            }

            if !cmd.should_run_on_version(&$ctx.build.build_version) {
                trace!($logger => "skipping command, shouldn't run on version {}", $ctx.build.build_version);
                continue;
            }

            info!($logger => "running command {:?}", cmd);
            $ctx.checked_exec(&opts.clone().cmd(&cmd.cmd), $logger)
                .await?;
        }

        Ok::<_, Error>(())
    }};
}

pub async fn run(ctx: &Context<'_>, logger: &mut BoxedCollector) -> Result<()> {
    info!(logger => "executing scripts");
    if let Some(config_script) = &ctx.build.recipe.configure_script {
        run_script!(
            "configure",
            config_script,
            &ctx.build.container_bld_dir,
            ctx,
            logger
        )?;
    } else {
        info!(logger => "no configure steps to run");
    }

    let build_script = &ctx.build.recipe.build_script;
    run_script!(
        "build",
        build_script,
        &ctx.build.container_bld_dir,
        ctx,
        logger
    )?;

    if let Some(install_script) = &ctx.build.recipe.install_script {
        run_script!(
            "install",
            install_script,
            &ctx.build.container_out_dir,
            ctx,
            logger
        )?;
    } else {
        info!(logger => "no install steps to run");
    }

    Ok(())
}
