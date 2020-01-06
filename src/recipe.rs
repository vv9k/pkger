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

    // Packages
    pub depends: Option<Vec<String>>,
    pub obsoletes: Option<Vec<String>>,
    pub conflicts: Option<Vec<String>>,
    pub provides: Option<Vec<String>>,
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

// # TODO
// Find a nicer way to generate this
pub fn generate_deb_control(info: &Info) -> String {
    let arch = match &info.arch[..] {
        "x86_64" => "amd64",
        // #TODO
        _ => "all",
    };
    trace!("generating control file");
    let mut control = format!(
        "Package: {}
Version: {}-{}
Architecture: {}
",
        &info.name, &info.version, &info.revision, &arch
    );
    control.push_str("Section: ");
    match &info.section {
        Some(section) => control.push_str(section),
        None => control.push_str("custom"),
    }
    control.push_str("\nPriority: ");
    match &info.priority {
        Some(priority) => control.push_str(priority),
        None => control.push_str("optional"),
    }
    control.push('\n');

    if let Some(dependencies) = &info.depends {
        control.push_str("Depends: ");
        let mut deps = String::new();
        for d in dependencies {
            trace!("adding dependency {}", d);
            deps.push_str(&format!("{}, ", d));
        }
        control.push_str(deps.trim_end_matches(", "));
        control.push('\n');
    }
    if let Some(conflicts) = &info.conflicts {
        control.push_str("Conflicts: ");
        let mut confs = String::new();
        for c in conflicts {
            trace!("adding conflict {}", c);
            confs.push_str(&format!("{}, ", c));
        }
        control.push_str(confs.trim_end_matches(", "));
        control.push('\n');
    }
    if let Some(obsoletes) = &info.obsoletes {
        control.push_str("Breaks: ");
        let mut obs = String::new();
        for o in obsoletes {
            trace!("adding obsolete {}", o);
            obs.push_str(&format!("{}, ", o));
        }
        control.push_str(obs.trim_end_matches(", "));
        control.push('\n');
    }
    if let Some(provides) = &info.provides {
        control.push_str("Provides: ");
        let mut prvds = String::new();
        for p in provides {
            trace!("adding provide {}", p);
            prvds.push_str(&format!("{}, ", p));
        }
        control.push_str(prvds.trim_end_matches(", "));
        control.push('\n');
    }

    control.push_str("Maintainer: ");
    match &info.maintainer {
        Some(maintainer) => control.push_str(maintainer),
        None => control.push_str("null <null@email.com>"),
    }

    control.push_str(&format!("\nDescription: {}\n", &info.description));

    trace!("{}", &control);
    control
}
