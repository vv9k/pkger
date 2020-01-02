use failure::Error;
use pkger::Pkger;
use structopt::StructOpt;

const DEFAULT_CONF_FILE: &'static str = "conf.toml";

#[derive(Debug, StructOpt)]
#[structopt(name = "pkger", about = "Creates RPM and DEB packages using docker")]
struct Opt {
    /// URL to dockers api
    #[structopt(short, long)]
    docker: String,
    /// Path to config file (default - "./conf.toml")
    #[structopt(short, long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    pretty_env_logger::init();
    let opts = Opt::from_args();
    let cfg = opts.config.unwrap_or(DEFAULT_CONF_FILE.to_string());

    let pkgr = Pkger::new(&opts.docker, &cfg)?;

    pkgr.build_recipe("curl").await?;

    Ok(())
}
