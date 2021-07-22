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
use tracing::error;

static DEFAULT_CONFIG_FILE: &str = ".pkger.yml";

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::from_args();

    // config
    let config_path = opts
        .config
        .clone()
        .unwrap_or_else(|| match dirs::home_dir() {
            Some(home_dir) => home_dir
                .join(DEFAULT_CONFIG_FILE)
                .to_string_lossy()
                .to_string(),
            None => DEFAULT_CONFIG_FILE.to_string(),
        });
    let result = Configuration::load(&config_path);
    if let Err(e) = &result {
        eprintln!(
            "Failed to read configuration file from `{}` - {}",
            config_path, e
        );
        process::exit(1);
    }
    let config = result.unwrap();

    fmt::setup_tracing(&opts, &config);

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
