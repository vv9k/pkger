use clap::Clap;

#[derive(Debug, Clap)]
#[clap(
    name = "pkger",
    version = "0.1.0",
    author = "Wojciech Kępka <wojciech@wkepka.dev>",
    about = "Creates RPM, DEB and other packages using Docker"
)]
pub struct Opts {
    /// URL to Docker daemon listening on a unix or tcp socket. An example could be
    /// `unix:///var/run/docker.socket` or a tcp uri `tcp://127.0.0.1:81`.
    #[clap(short, long)]
    pub docker: Option<String>,
    /// Recipes to build. If empty all recipes in the `recipes_dir` directory will be built.
    pub recipes: Vec<String>,
    /// Specify the images on which to build the recipes. Only those recipes that have one or more
    /// of the images provided as this argument are going to get built.
    #[clap(short, long)]
    pub images: Option<Vec<String>>,
    /// Path to the config file (default - "./conf.toml").
    #[clap(short, long)]
    pub config: Option<String>,
    /// Surpress all output.
    #[clap(short, long)]
    pub quiet: bool,
}

impl Opts {
    pub fn from_args() -> Self {
        Clap::parse()
    }
}
