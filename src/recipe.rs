use super::*;
#[derive(Deserialize, Debug)]
pub struct Recipe {
    pub info: Info,
    pub build: Build,
    pub install: Install,
}
impl Recipe {
    pub fn new(entry: DirEntry) -> Result<Recipe, Error> {
        let mut path = entry.path();
        path.push("recipe.toml");
        Ok(toml::from_str::<Recipe>(&fs::read_to_string(&path)?)?)
    }
}
pub type Recipes = HashMap<String, Recipe>;
#[derive(Deserialize, Debug)]
pub struct Info {
    // General
    pub name: String,
    pub version: String,
    pub arch: String,
    pub revision: String,
    pub description: String,
    pub license: String,
    pub source: String,
    pub images: Vec<String>,

    // Git repository as source
    pub git: Option<String>,

    // Debian based specific packages
    pub depends: Option<Vec<String>>,
    pub obsoletes: Option<Vec<String>>,
    pub conflicts: Option<Vec<String>>,
    pub provides: Option<Vec<String>>,

    // RedHat based specific packages
    pub depends_rh: Option<Vec<String>>,
    pub obsoletes_rh: Option<Vec<String>>,
    pub conflicts_rh: Option<Vec<String>>,
    pub provides_rh: Option<Vec<String>>,

    // Directories to exclude when creating the package
    pub exclude: Option<Vec<String>>,

    // Only Debian based
    pub maintainer: Option<String>,
    pub section: Option<String>,
    pub priority: Option<String>,
}
#[derive(Deserialize, Debug)]
pub struct Build {
    pub steps: Vec<String>,
}
#[derive(Deserialize, Debug)]
pub struct Install {
    pub steps: Vec<String>,
    pub destdir: String,
}
