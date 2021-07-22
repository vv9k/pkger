mod app;
mod config;
mod fmt;
mod gen;
mod job;
mod opts; // generate

use app::Application;
use config::Configuration;
use opts::Opts;

use pkger_core::{Error, Result};

use std::process;
use tracing::{error, trace, warn};

static DEFAULT_CONFIG_FILE: &str = ".pkger.yml";

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::from_args();

    fmt::setup_tracing(&opts);

    trace!(opts = ?opts);

    // config
    let config_path = opts.config.clone().unwrap_or_else(|| {
        match dirs::home_dir() {
            Some(home_dir) => {
                home_dir.join(DEFAULT_CONFIG_FILE).to_string_lossy().to_string()
            }
            None => {
                warn!(path = %DEFAULT_CONFIG_FILE, "current user has no home directory, using default");
                DEFAULT_CONFIG_FILE.to_string()
            }
        }
    });
    trace!(config_path = %config_path);
    let result = Configuration::load(&config_path);
    if let Err(e) = &result {
        error!(reason = %e, config_path = %config_path, "failed to read config file");
        process::exit(1);
    }
    let config = result.unwrap();
    trace!(config = ?config);

    let mut app = match Application::new(config) {
        Ok(app) => app,
        Err(e) => {
            error!(reason = %e, "failed to initialize pkger");
            process::exit(1);
        }
    };

    if let Err(reason) = app.process_opts(opts).await {
        let reason = format!("\nError: {:?}", reason);
        error!(%reason, "execution failed");
        process::exit(1);
    }
    Ok(())
}
