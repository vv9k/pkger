use clap::{Clap, Subcommand};
use std::path::PathBuf;
#[derive(Debug, Clap)]
#[clap(
    name = "pkger",
    version = "0.5.0",
    author = "Wojciech Kępka <wojciech@wkepka.dev>",
    about = "Creates RPM, DEB and other packages using Docker"
)]
pub struct Opts {
    #[clap(short, long)]
    /// Suppress all output from containers.
    pub quiet: bool,
    #[clap(short, long)]
    /// Enable debug output.
    pub debug: bool,
    #[clap(short, long)]
    /// Enable trace output.
    pub trace: bool,
    #[clap(long)]
    /// Filter string that instruments the formatter which fields should be displayed. Each
    /// character of the string corresponds to a field. Available fields to show are: D - Date, F -
    /// Fields, S - Spans. L - Levels is on by default, use L to turn it off. All characters can be
    /// upper or lower case, the order doesn't matter, duplicates and errors are silently ignored.
    pub filter: Option<String>,
    #[clap(short, long)]
    /// Path to the config file (default - "~/.pkger.yml").
    pub config: Option<String>,

    #[clap(subcommand)]
    /// Subcommand to run
    pub command: Command,
}

impl Opts {
    pub fn from_args() -> Self {
        Clap::parse()
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Runs a build creating specified packages on target platforms.
    Build(BuildOpts),
    /// Creates a directory with a recipe generated from provided arguments.
    GenRecipe(Box<GenRecipeOpts>),
    /// Lists the specified objects like images.
    List {
        #[clap(subcommand)]
        /// An object to list like `image`, `recipe` or `package`.
        object: ListObject,
    },
    /// Deletes the cache files with image state.
    CleanCache,
    /// Edit a recipe or an image.
    Edit {
        #[clap(subcommand)]
        /// An object to edit like `image`, `recipe` or `config`.
        object: EditObject,
    },
    /// Initializes required directories and a configuration file at specified or default locations.
    Init {
        #[clap(short, long)]
        /// Override the default location to which the configuration file will be saved.
        config: Option<PathBuf>,
        #[clap(short, long)]
        /// Override the default location of custom images.
        images: Option<PathBuf>,
        #[clap(short, long)]
        /// Override the default location of output packages.
        output: Option<PathBuf>,
        #[clap(short, long)]
        /// Override the default location of recipes.
        recipes: Option<PathBuf>,
        #[clap(short, long)]
        /// Filter string that sets which fields are displayed during builds.
        filter: Option<String>,
        #[clap(short, long)]
        /// URL to Docker daemon listening on a unix or tcp socket. An example could be
        /// `unix:///var/run/docker.sock` or a tcp uri `tcp://127.0.0.1:81`. By default, on a unix host
        /// pkger will try to connect to a unix socket at locations like `/var/run/docker.sock` or
        /// `/run/docker.sock`. On non-unix operating systems like windows a TCP connection to
        /// `127.0.0.1:8080` is used.
        docker: Option<String>,
        #[clap(long)]
        /// Absolute path to the GPG key used to sign packages.
        gpg_key: Option<PathBuf>,
        #[clap(long)]
        /// The value of the `Name` field of the GPG key `gpg_key`.
        gpg_name: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum EditObject {
    Recipe { name: String },
    Image { name: String },
    Config,
}

#[derive(Debug, Subcommand)]
pub enum ListObject {
    Images,
    Recipes,
    Packages {
        #[clap(short, long)]
        images: Option<Vec<String>>,
    },
}

#[derive(Debug, Clap)]
pub struct BuildOpts {
    /// Recipes to build. If empty all recipes in the `recipes_dir` directory will be built.
    pub recipes: Vec<String>,
    #[clap(short, long)]
    /// A list of targets to build like `rpm deb pkg`. All images needed to build each recipe for
    /// each target will be created on the go. When this flag is provided all custom images and
    /// image targets defined in recipes will be ignored.
    pub simple: Option<Vec<String>>,
    #[clap(short, long)]
    /// Specify the images on which to build the recipes. Only those recipes that have one or more
    /// of the images provided as this argument are going to get built. This flag is ignored when
    /// `targets` is specified.
    pub images: Option<Vec<String>>,
    #[clap(long)]
    /// URL to Docker daemon listening on a unix or tcp socket. An example could be
    /// `unix:///var/run/docker.sock` or a tcp uri `tcp://127.0.0.1:81`. By default, on a unix host
    /// pkger will try to connect to a unix socket at locations like `/var/run/docker.sock` or
    /// `/run/docker.sock`. On non-unix operating systems like windows a TCP connection to
    /// `127.0.0.1:8080` is used.
    pub docker: Option<String>,

    #[clap(long, short)]
    /// If set to true, all recipes will be built.
    pub all: bool,

    #[clap(long)]
    /// Disable signing packages. This option only has effect when signing is enabled in
    /// the configuration.
    pub no_sign: bool,
}

#[derive(Debug, Clap)]
pub struct GenRecipeOpts {
    /// Name of the recipe to generate
    pub name: String,
    /// Parent directory in which a directory with the recipe should be created. If no path is
    /// provided the recipe will be printed to stdout.
    pub output_dir: Option<PathBuf>,
    #[clap(long)]
    pub version: Option<String>,
    #[clap(long)]
    pub description: Option<String>,
    #[clap(long)]
    pub license: Option<String>,

    #[clap(long)]
    pub maintainer: Option<String>,
    #[clap(long)]
    /// The website of the package
    pub url: Option<String>,
    #[clap(long)]
    pub arch: Option<String>,
    #[clap(long)]
    /// http/https or file system source pointing to a tar archive or some other file
    pub source: Option<String>,
    #[clap(long)]
    /// Git repository as source
    pub git_url: Option<String>,
    #[clap(long)]
    /// Git branch if `git_url` is also provided
    pub git_branch: Option<String>,
    #[clap(long)]
    /// Whether to install default dependencies before build
    pub skip_default_deps: Option<bool>,
    #[clap(long)]
    /// Directories to exclude when creating the package
    pub exclude: Option<Vec<String>>,
    #[clap(long)]
    /// Group in RPM and PKG or section in DEB build
    pub group: Option<String>,
    #[clap(long)]
    /// The release number. This is usually a positive integer number that allows to differentiate
    /// between consecutive builds of the same version of a package
    pub release: Option<String>,
    #[clap(long)]
    /// Used to force the package to be seen as newer than any previous version with a lower epoch
    pub epoch: Option<String>,

    #[clap(long)]
    pub build_depends: Option<Vec<String>>,

    #[clap(long)]
    pub depends: Option<Vec<String>>,
    #[clap(long)]
    pub conflicts: Option<Vec<String>>,
    #[clap(long)]
    pub provides: Option<Vec<String>>,

    #[clap(long)]
    pub patches: Option<Vec<String>>,

    #[clap(long)]
    /// A comma separated list of k=v entries like:
    /// `HTTP_PROXY=proxy.corp.local,PATH=$PATH:/opt/dev/bin`
    pub env: Option<String>,

    #[clap(long)]
    /// A list of packages that this packages replaces. Applies to DEB and PKG
    pub replaces: Option<Vec<String>>,

    // Only DEB
    #[clap(long)]
    /// Only applies to DEB build
    pub priority: Option<String>,
    #[clap(long)]
    /// Only applies to DEB build
    pub installed_size: Option<String>,
    #[clap(long)]
    /// Only applies to DEB build
    pub built_using: Option<String>,
    #[clap(long)]
    /// Only applies to DEB build
    pub essential: Option<bool>,

    #[clap(long)]
    /// Only applies to DEB build
    pub pre_depends: Option<Vec<String>>,
    #[clap(long)]
    /// Only applies to DEB build
    pub recommends: Option<Vec<String>>,
    #[clap(long)]
    /// Only applies to DEB build
    pub suggests: Option<Vec<String>>,
    #[clap(long)]
    /// Only applies to DEB build
    pub breaks: Option<Vec<String>>,
    #[clap(long)]
    /// Only applies to DEB build
    pub enchances: Option<Vec<String>>,

    // Only RPM
    #[clap(long)]
    /// Only applies to RPM
    pub obsoletes: Option<Vec<String>>,
    #[clap(long)]
    /// Only applies to RPM
    pub vendor: Option<String>,
    #[clap(long)]
    /// Only applies to RPM
    pub icon: Option<String>,
    #[clap(long)]
    /// Only applies to RPM
    pub summary: Option<String>,
    #[clap(long)]
    /// Only applies to RPM
    pub config_noreplace: Option<String>,

    // Only PKG
    #[clap(long)]
    /// The name of the .install script to be included in the package. Only applies to PKG
    pub install_script: Option<String>,
    #[clap(long)]
    /// A list of files that can contain user-made changes and should be preserved during upgrade
    /// or removal of a package. Only applies to PKG
    pub backup_files: Option<Vec<String>>,
    #[clap(long)]
    /// Optional dependencies needed for full functionality of the package. Only applies to PKG
    pub optdepends: Option<Vec<String>>,
}
