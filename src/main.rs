use failure::Error;
use log::*;
use pkger::Pkger;
use structopt::StructOpt;

const DEFAULT_CONF_FILE: &'static str = "conf.toml";

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
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    pretty_env_logger::init();
    let opts = Opt::from_args();
    trace!("{:?}", opts);
    let cfg = opts.config.unwrap_or(DEFAULT_CONF_FILE.to_string());
    let pkger = Pkger::new(&opts.docker, &cfg)?;
    trace!("{:?}", pkger);

    for recipe in opts.recipes.iter() {
        pkger.build_recipe(&recipe).await.unwrap()
    }

    Ok(())
}
