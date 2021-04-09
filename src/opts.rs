use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "pkger", about = "Creates RPM and DEB packages using docker")]
pub struct Opts {
    /// URL to dockers api
    #[structopt(short, long)]
    pub docker: Option<String>,
    /// Recipes to build
    pub recipes: Vec<String>,
    /// Path to config file (default - "./conf.toml")
    #[structopt(short, long)]
    pub config: Option<String>,
    /// No output printed to stdout
    #[structopt(short, long)]
    pub quiet: bool,
}

impl Opts {
    pub fn from_args() -> Self {
        StructOpt::from_args()
    }
}
