use crate::completions::Shell;
use clap::Parser;
use std::path::PathBuf;

pub const APP_NAME: &str = "pkger";

#[derive(Debug, Parser)]
#[clap(
    name = APP_NAME,
    version = "0.9.0",
    about = "Creates RPM, DEB and other packages using Docker"
)]
pub struct Opts {
    #[clap(short, long)]
    /// Display only errors and warnings.
    pub quiet: bool,
    #[clap(short, long)]
    /// Enable debug output.
    pub debug: bool,
    #[clap(short, long)]
    /// Enable trace output.
    pub trace: bool,
    #[clap(short, long)]
    /// Path to the config file (default - "~/.pkger.yml").
    pub config: Option<String>,

    #[clap(short, long)]
    /// Directory for log files. All output will be redirected to files in this directory.
    pub log_dir: Option<PathBuf>,

    #[clap(subcommand)]
    /// Subcommand to run
    pub command: Command,

    #[clap(short, long)]
    /// URL to container runtime daemon listening on a unix or tcp socket. An example could be
    /// `unix:///var/run/docker.sock` or a tcp uri `tcp://127.0.0.1:81`. By default, on a unix host
    /// pkger will try to connect to a unix socket at locations like `/var/run/docker.sock` or
    /// `/run/docker.sock`. On non-unix operating systems like windows a TCP connection to
    /// `127.0.0.1:8080` is used.
    pub runtime_uri: Option<String>,
    #[clap(short, long)]
    /// If provided pkger will try to use podman instead of docker as a container runtime.
    pub podman: bool,

    #[clap(long)]
    pub no_color: bool,
}

impl Opts {
    pub fn from_args() -> Self {
        Opts::parse()
    }
}

#[derive(Debug, Parser)]
pub enum Command {
    #[clap(aliases = &["b", "bld"])]
    /// Runs a build creating specified packages on target platforms.
    Build(BuildOpts),
    #[clap(alias = "ls")]
    /// Lists the specified objects like images.
    List {
        #[clap(subcommand)]
        /// An object to list like `image`, `recipe` or `package`.
        object: ListObject,
        #[clap(short, long)]
        /// Disable colored output.
        raw: bool,
        #[clap(short, long)]
        /// Should the output be more verbose and include fields like version, arch...
        verbose: bool,
    },
    #[clap(alias = "cc")]
    /// Deletes the cache files with image state.
    CleanCache,
    #[clap(alias = "e")]
    /// Edit a recipe or an image.
    Edit {
        #[clap(subcommand)]
        /// An object to edit like `image`, `recipe` or `config`.
        object: EditObject,
    },
    #[clap(alias = "n")]
    /// Generate a new image or recipe.
    New {
        #[clap(subcommand)]
        /// An object to create like `image` or `recipe`.
        object: NewObject,
    },
    #[clap(alias = "cp")]
    /// Copy an image or a recipe
    Copy {
        #[clap(subcommand)]
        /// An object to copy like `image` or `recipe`.
        object: CopyObject,
    },
    #[clap(alias = "rm")]
    /// Remove images or recipes
    Remove {
        #[clap(subcommand)]
        /// An object to remove like `image` or `recipe`.
        object: RemoveObject,
        #[clap(short, long)]
        /// Should there be any output like errors
        quiet: bool,
    },
    /// Initializes required directories and a configuration file at specified or default locations.
    Init(InitOpts),
    /// Prints completions for the specified shell
    PrintCompletions(CompletionsOpts),
    /// Run various checks to verify health of the setup
    Check {
        #[clap(subcommand)]
        /// An object to check
        object: CheckObject,
    },
}

#[derive(Debug, Parser)]
pub struct InitOpts {
    #[clap(short, long)]
    /// Override the default location to which the configuration file will be saved.
    pub config: Option<PathBuf>,
    #[clap(short, long)]
    /// Override the default location of custom images.
    pub images: Option<PathBuf>,
    #[clap(short, long)]
    /// Override the default location of output packages.
    pub output: Option<PathBuf>,
    #[clap(short, long)]
    /// Override the default location of recipes.
    pub recipes: Option<PathBuf>,
    #[clap(long)]
    /// Absolute path to the GPG key used to sign packages.
    pub gpg_key: Option<PathBuf>,
    #[clap(long)]
    /// The value of the `Name` field of the GPG key `gpg_key`.
    pub gpg_name: Option<String>,
}

#[derive(Debug, Parser)]
pub enum CheckObject {
    #[clap(aliases = &["conn", "con"])]
    /// Verify the connection to the container runtime daemon.
    Connection,
}

#[derive(Debug, Parser)]
pub enum EditObject {
    #[clap(alias = "rcp")]
    Recipe { name: String },
    #[clap(alias = "img")]
    Image { name: String },
    #[clap(alias = "cfg")]
    Config,
}

#[derive(Debug, Parser)]
pub enum ListObject {
    #[clap(aliases = &["image", "img"])]
    Images,
    #[clap(aliases = &["recipe", "rcp"])]
    Recipes,
    #[clap(aliases = &["package", "pkg"])]
    Packages {
        #[clap(short, long)]
        #[clap(multiple_values = true)]
        images: Option<Vec<String>>,
    },
}

#[derive(Debug, Parser)]
pub enum CopyObject {
    #[clap(alias = "rcp")]
    /// Copy a recipe
    Recipe {
        /// Source recipe to copy
        source: String,
        /// What to call the output recipe
        dest: String,
    },
    #[clap(alias = "img")]
    /// Copy an image
    Image {
        /// Source image to copy
        source: String,
        /// What to call the output image
        dest: String,
    },
}

#[derive(Debug, Parser)]
pub enum NewObject {
    #[clap(alias = "rcp")]
    Recipe(Box<GenRecipeOpts>),
    #[clap(alias = "img")]
    Image {
        /// The name of the image to create.
        name: String,
    },
}

#[derive(Debug, Parser)]
pub enum RemoveObject {
    #[clap(aliases = &["recipe", "rcp"])]
    /// Remove recipes
    Recipes {
        /// One or more recipes to delete.
        names: Vec<String>,
    },
    #[clap(aliases = &["image", "img"])]
    /// Remove images
    Images {
        /// One or more images to delete.
        names: Vec<String>,
    },
}

#[derive(Debug, Parser)]
pub struct BuildOpts {
    /// Recipes to build. If empty all recipes in the `recipes_dir` directory will be built.
    pub recipes: Vec<String>,
    #[clap(short, long)]
    #[clap(multiple_values = true)]
    /// A list of targets to build like `rpm deb pkg`. All images needed to build each recipe for
    /// each target will be created on the go. When this flag is provided all custom images and
    /// image targets defined in recipes will be ignored.
    pub simple: Option<Vec<String>>,
    #[clap(short, long)]
    #[clap(multiple_values = true)]
    /// Specify the images on which to build the recipes. Only those recipes that have one or more
    /// of the images provided as this argument are going to get built. This flag is ignored when
    /// `targets` is specified.
    pub images: Option<Vec<String>>,

    #[clap(long, short)]
    /// If set to true, all recipes will be built.
    pub all: bool,

    #[clap(long)]
    /// Disable signing packages. This option only has effect when signing is enabled in
    /// the configuration.
    pub no_sign: bool,

    #[clap(short, long)]
    /// Override output directory specified in the configuration
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Parser)]
pub struct GenRecipeOpts {
    /// Name of the recipe to generate
    pub name: String,

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
    /// http/https or file system source/sources pointing to a tar archive or some other file
    pub source: Option<Vec<String>>,
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
    #[clap(multiple_values = true)]
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
    #[clap(multiple_values = true)]
    pub build_depends: Option<Vec<String>>,

    #[clap(long)]
    #[clap(multiple_values = true)]
    pub depends: Option<Vec<String>>,
    #[clap(long)]
    #[clap(multiple_values = true)]
    pub conflicts: Option<Vec<String>>,
    #[clap(long)]
    #[clap(multiple_values = true)]
    pub provides: Option<Vec<String>>,

    #[clap(long)]
    #[clap(multiple_values = true)]
    pub patches: Option<Vec<String>>,

    #[clap(long)]
    /// A comma separated list of k=v entries like:
    /// `HTTP_PROXY=proxy.corp.local,PATH=$PATH:/opt/dev/bin`
    pub env: Option<String>,

    #[clap(long)]
    #[clap(multiple_values = true)]
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
    #[clap(multiple_values = true)]
    /// Only applies to DEB build
    pub pre_depends: Option<Vec<String>>,
    #[clap(long)]
    #[clap(multiple_values = true)]
    /// Only applies to DEB build
    pub recommends: Option<Vec<String>>,
    #[clap(long)]
    #[clap(multiple_values = true)]
    /// Only applies to DEB build
    pub suggests: Option<Vec<String>>,
    #[clap(long)]
    #[clap(multiple_values = true)]
    /// Only applies to DEB build
    pub breaks: Option<Vec<String>>,
    #[clap(long)]
    #[clap(multiple_values = true)]
    /// Only applies to DEB build
    pub enchances: Option<Vec<String>>,

    // Only RPM
    #[clap(long)]
    #[clap(multiple_values = true)]
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
    #[clap(multiple_values = true)]
    /// A list of files that can contain user-made changes and should be preserved during upgrade
    /// or removal of a package. Only applies to PKG
    pub backup_files: Option<Vec<String>>,
    #[clap(long)]
    /// Optional dependencies needed for full functionality of the package. Only applies to PKG
    pub optdepends: Option<Vec<String>>,
}

#[derive(Debug, Parser)]
pub struct CompletionsOpts {
    /// A shell for which to print completions. Available shells are: bash, elvish, fish,
    /// powershell, zsh
    pub shell: Shell,
}
