#[macro_use]
extern crate pkger_core;

use std::fs;
use std::process;
use std::time::SystemTime;

use app::Application;
use opts::Opts;
use pkger_core::config::Configuration;
use pkger_core::log::{self, error};
use pkger_core::{ErrContext, Error, Result};

mod app;
mod completions;
mod config;
mod gen;
mod job;
mod metadata;
mod opts;
mod table;

static DEFAULT_CONFIG_FILE: &str = ".pkger.yml";

macro_rules! exit {
    ($($args:tt)*) => {{
        error!($($args)*);
        process::exit(1);
    }};

}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::from_args();

    pretty_env_logger::init();

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if let opts::Command::Init(init_opts) = opts.command {
        let config_dir = dirs::config_dir().context("missing config directory")?;
        let pkger_dir = config_dir.join("pkger");
        let recipes_dir = init_opts
            .recipes
            .unwrap_or_else(|| pkger_dir.join("recipes"));
        let output_dir = init_opts.output.unwrap_or_else(|| pkger_dir.join("output"));
        let images_dir = init_opts.images.unwrap_or_else(|| pkger_dir.join("images"));
        let config_path = init_opts
            .config
            .unwrap_or_else(|| config_dir.join(DEFAULT_CONFIG_FILE));

        if !images_dir.exists() {
            println!("creating images directory ~> `{}`", images_dir.display());
            fs::create_dir_all(&images_dir).context("failed to create images dir")?;
        }
        if !output_dir.exists() {
            println!("creating output directory ~> `{}`", output_dir.display());
            fs::create_dir_all(&output_dir).context("failed to create output dir")?;
        }
        if !recipes_dir.exists() {
            println!("creating recipes directory ~> `{}`", recipes_dir.display());
            fs::create_dir_all(&recipes_dir).context("failed to create recipes dir")?;
        }

        let cfg = Configuration {
            recipes_dir,
            output_dir,
            images_dir: Some(images_dir),
            log_dir: None,
            runtime_uri: opts.runtime_uri,
            podman: opts.podman,
            gpg_key: init_opts.gpg_key,
            gpg_name: init_opts.gpg_name,
            ssh: None,
            images: vec![],
            path: config_path,
            custom_simple_images: None,
            no_color: false,
        };

        if cfg.path.exists() {
            let mut line = String::new();
            loop {
                println!("configuration file already exists, overwrite? y/n");
                std::io::stdin()
                    .read_line(&mut line)
                    .context("failed to read input from user")?;
                match line.trim() {
                    "y" => break,
                    "n" => {
                        println!("exiting...");
                        process::exit(1)
                    }
                    _ => continue,
                }
            }
        }
        println!("saving configuration ~> `{}`", cfg.path.display());
        cfg.save()?;
        process::exit(0);
    }

    // config
    let config_path = opts
        .config
        .clone()
        .unwrap_or_else(|| match dirs::config_dir() {
            Some(config_dir) => config_dir
                .join(DEFAULT_CONFIG_FILE)
                .to_string_lossy()
                .to_string(),
            None => DEFAULT_CONFIG_FILE.to_string(),
        });
    let result = Configuration::load(&config_path).context("failed to load configuration file");
    if let Err(e) = &result {
        exit!("execution failed, reason: {:?}", e);
    }
    let config = result.unwrap();

    let mut logger_config = if let Some(p) = &opts.log_dir {
        log::Config::file(p.join(format!("pkger-{}.log", timestamp)))
    } else if let Some(p) = &config.log_dir {
        log::Config::file(p.join(format!("pkger-{}.log", timestamp)))
    } else {
        log::Config::stdout()
    };

    let disable_color = opts.no_color || config.no_color;
    if disable_color {
        logger_config = logger_config.no_color(true);
        if let Ok(mut log) = log::GLOBAL_OUTPUT_COLLECTOR.try_write() {
            log.set_override(false);
        }
    }

    let mut logger = match logger_config
        .as_collector()
        .context("failed to initialize global output collector")
    {
        Ok(config) => config,
        Err(e) => exit!("execution failed, reason: {:?}", e),
    };

    if opts.trace {
        logger.set_level(log::Level::Trace);
    } else if opts.debug {
        logger.set_level(log::Level::Debug);
    } else if opts.quiet {
        logger.set_level(log::Level::Warn);
    }

    trace!(logger => "{:#?}", opts);
    trace!(logger => "{:#?}", config);

    let mut app =
        match Application::new(config, &opts, &mut logger).context("failed to initialize pkger") {
            Ok(app) => app,
            Err(e) => exit!("execution failed, reason: {:?}", e),
        };

    if let Err(e) = app.process_opts(opts, &mut logger).await {
        exit!("execution failed, reason: {:?}", e);
    }
    Ok(())
}
