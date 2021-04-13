use crate::cleanup;
use crate::image::{Image, ImageState};
use crate::recipe::{BuildTarget, Recipe};
use crate::Result;

use futures::StreamExt;
use moby::{tty::TtyChunk, Container, ExecContainerOptions};
use std::path::{Path, PathBuf};
use std::str;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, info_span, trace, Instrument};

pub struct BuildContainerCtx<'job> {
    container: Container<'job>,
    recipe: &'job Recipe,
    image: &'job Image,
    is_running: Arc<AtomicBool>,
    _bld_dir: PathBuf,
    _out_dir: PathBuf,
    _target: BuildTarget,
}

impl<'job> BuildContainerCtx<'job> {
    pub fn new(
        container: Container<'job>,
        recipe: &'job Recipe,
        image: &'job Image,
        is_running: Arc<AtomicBool>,
        target: BuildTarget,
        bld_dir: &Path,
        out_dir: &Path,
    ) -> BuildContainerCtx<'job> {
        BuildContainerCtx {
            recipe,
            image,
            container,
            is_running,
            _target: target,
            _bld_dir: bld_dir.to_path_buf(),
            _out_dir: out_dir.to_path_buf(),
        }
    }

    pub async fn cleanup(&self) -> Result<()> {
        let span = info_span!("cleanup");
        let _enter = span.enter();

        trace!("stopping container");
        self.container
            .stop(None)
            .instrument(span.clone())
            .await
            .map_err(|e| anyhow!("failed to stop container - {}", e))?;

        trace!("deleting container");
        self.container
            .delete()
            .instrument(span.clone())
            .await
            .map_err(|e| anyhow!("failed to delete container - {}", e))
    }

    pub async fn cleanup_if_exit(&self) -> Result<bool> {
        let span = info_span!("check-is-running");
        let _enter = span.enter();
        if !self.is_running.load(Ordering::SeqCst) {
            trace!("not running");

            return self.cleanup().await.map(|_| true);
        }

        Ok(false)
    }

    pub async fn container_exec<S: AsRef<str>>(&self, cmd: S) -> Result<()> {
        let span = info_span!("container-exec");
        let _enter = span.enter();

        debug!(cmd = %cmd.as_ref(), "executing");

        let opts = ExecContainerOptions::builder()
            .cmd(vec!["/bin/sh", "-c", cmd.as_ref()])
            .attach_stdout(true)
            .attach_stderr(true)
            .build();

        let mut stream = self.container.exec(&opts);

        while let Some(result) = stream.next().instrument(span.clone()).await {
            cleanup!(self, span);
            match result? {
                TtyChunk::StdOut(chunk) => {
                    info!("{}", str::from_utf8(&chunk)?.trim_end_matches("\n"));
                }
                TtyChunk::StdErr(chunk) => {
                    error!("{}", str::from_utf8(&chunk)?.trim_end_matches("\n"));
                }
                _ => unreachable!(),
            }
        }
        Ok(())
    }

    pub async fn install_deps(&self, state: &ImageState) -> Result<()> {
        let span = info_span!("install-deps", container = %self.container.id());
        let _enter = span.enter();

        info!("installing dependencies");
        let pkg_mngr = state.os.package_manager();
        let deps = if let Some(deps) = &self.recipe.metadata.build_depends {
            deps.resolve_names(&state.image)
        } else {
            vec![]
        };

        if deps.is_empty() {
            trace!("no dependencies to install");
            return Ok(());
        }

        let deps = deps.join(" ");
        trace!(deps = %deps, "resolved dependency names");

        let cmd = format!(
            "{} {} {}",
            pkg_mngr.as_ref(),
            pkg_mngr.install_args().join(" "),
            deps,
        );
        trace!(command = %cmd, "installing with");

        self.container_exec(cmd).instrument(span.clone()).await
    }

    pub async fn execute_scripts(&self) -> Result<()> {
        let span = info_span!("exec-scripts", container = %self.container.id());
        let _enter = span.enter();

        if let Some(config_script) = &self.recipe.configure_script {
            info!("executing config scripts");
            for cmd in &config_script.steps {
                trace!(command = %cmd.cmd, "processing");
                if !cmd.images.is_empty() {
                    trace!(images = ?cmd.images, "only execute on");
                    if !cmd.images.contains(&self.image.name) {
                        trace!(image = %self.image.name, "not found, skipping");
                        continue;
                    }
                }
                trace!(command = %cmd.cmd, "running");
                self.container_exec(&cmd.cmd)
                    .instrument(span.clone())
                    .await?;
            }
        }

        info!("executing build scripts");
        for cmd in &self.recipe.build_script.steps {
            trace!(command = %cmd.cmd, "processing");
            if !cmd.images.is_empty() {
                trace!(images = ?cmd.images, "only execute on");
                if !cmd.images.contains(&self.image.name) {
                    trace!(image = %self.image.name, "not found, skipping");
                    continue;
                }
            }
            trace!(command = %cmd.cmd, "running");
            self.container_exec(&cmd.cmd)
                .instrument(span.clone())
                .await?;
        }

        if let Some(install_script) = &self.recipe.install_script {
            info!("executing install scripts");
            for cmd in &install_script.steps {
                trace!(command = %cmd.cmd, "processing");
                if !cmd.images.is_empty() {
                    trace!(images = ?cmd.images, "only execute on");
                    if !cmd.images.contains(&self.image.name) {
                        trace!(image = %self.image.name, "not found, skipping");
                        continue;
                    }
                }
                trace!(command = %cmd.cmd, "running");
                self.container_exec(&cmd.cmd)
                    .instrument(span.clone())
                    .await?;
            }
        }

        Ok(())
    }
}
