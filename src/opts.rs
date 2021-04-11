use clap::Clap;

#[derive(Debug, Clap)]
#[clap(name = "pkger", about = "Creates RPM and DEB packages using docker")]
pub struct Opts {
    /// URL to dockers api
    #[clap(short, long)]
    pub docker: Option<String>,
    /// Recipes to build
    pub recipes: Vec<String>,
    /// Path to config file (default - "./conf.toml")
    #[clap(short, long)]
    pub config: Option<String>,
    /// No output printed to stdout
    #[clap(short, long)]
    pub quiet: bool,
}

impl Opts {
    pub fn from_args() -> Self {
        Clap::parse()
    }
}
