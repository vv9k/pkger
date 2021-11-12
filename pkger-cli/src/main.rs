#[macro_use]
extern crate pkger_core;

use std::fs;
use std::process;

use tracing::error;

use app::Application;
use config::Configuration;
use opts::Opts;
use pkger_core::{ErrContext, Error, Result};

mod app;
mod completions;
mod config;
mod fmt;
mod gen;
mod job;
mod metadata;
mod opts;
mod table;

static DEFAULT_CONFIG_FILE: &str = ".pkger.yml";

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::from_args();

    if let opts::Command::Init(opts) = opts.command {
        let config_dir = dirs::config_dir().context("missing config directory")?;
        let pkger_dir = config_dir.join("pkger");
        let recipes_dir = opts.recipes.unwrap_or_else(|| pkger_dir.join("recipes"));
        let output_dir = opts.output.unwrap_or_else(|| pkger_dir.join("output"));
        let images_dir = opts.images.unwrap_or_else(|| pkger_dir.join("images"));
        let config_path = opts
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
            filter: opts.filter,
            docker: opts.docker,
            gpg_key: opts.gpg_key,
            gpg_name: opts.gpg_name,
            ssh: None,
            images: vec![],
            path: config_path,
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
    let result = Configuration::load(&config_path);
    if let Err(e) = &result {
        eprintln!("`{}` - {:?}", config_path, e);
        process::exit(1);
    }
    let config = result.unwrap();

    fmt::setup_tracing(&opts, &config);

    let mut app = match Application::new(config) {
        Ok(app) => app,
        Err(error) => {
            error!(reason = %format!("{:?}", error), "failed to initialize pkger");
            process::exit(1);
        }
    };

    if let Err(error) = app.process_opts(opts).await {
        error!(reason = %format!("{:?}", error), "execution failed");
        process::exit(1);
    }
    Ok(())
}
