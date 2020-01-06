use failure::Error;
use log::*;
use pkger::Pkger;
use std::env;
use structopt::StructOpt;

const DEFAULT_CONF_FILE: &str = "conf.toml";

#[derive(Debug, StructOpt)]
#[structopt(name = "pkger", about = "Creates RPM and DEB packages using docker")]
struct Opt {
    /// URL to dockers api
    #[structopt(short, long)]
    docker: String,
    /// Recipes to build
    recipes: Vec<String>,
    /// Path to config file (default - "./conf.toml")
    #[structopt(short, long)]
    config: Option<String>,
    /// No output printed to stdout
    #[structopt(short, long)]
    quiet: bool,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let opts = Opt::from_args();
    if !opts.quiet {
        if env::var_os("RUST_LOG").is_none() {
            env::set_var("RUST_LOG", "pkger=info");
        }
        pretty_env_logger::init();
    }
    trace!("{:?}", opts);
    let cfg = opts.config.unwrap_or_else(|| DEFAULT_CONF_FILE.to_string());
    let pkger = Pkger::new(&opts.docker, &cfg)?;
    trace!("{:?}", pkger);

    for recipe in opts.recipes.iter() {
        pkger.build_recipe(&recipe).await.unwrap()
    }

    Ok(())
}
