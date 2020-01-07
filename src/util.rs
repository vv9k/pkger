//! Helper functions that don't fit anywhere else
use super::*;

pub fn find_penultimate_ancestor<P: AsRef<Path>>(path: P) -> PathBuf {
    trace!("finding parent of {}", path.as_ref().display());
    let mut ancestors = path.as_ref().ancestors();
    loop {
        match ancestors.next() {
            Some(ancestor) => {
                if ancestors.next() == Some(Path::new("")) {
                    trace!("found {}", ancestor.display());
                    return ancestor.to_path_buf();
                }
            }
            None => return PathBuf::from(""),
        }
    }
}

pub fn should_include<P: AsRef<Path>>(path: P, excludes: &[String]) -> bool {
    trace!("checking if {} should be included", path.as_ref().display());
    for e in excludes {
        if path.as_ref().starts_with(e) {
            return false;
        }
    }
    true
}

pub fn parse_env_vars(vars: &Option<toml::value::Table>) -> Vec<String> {
    let mut env = Vec::new();
    if let Some(_vars) = vars {
        for (k, v) in _vars.into_iter() {
            env.push(format!("{}={}", k, v));
        }
    }
    env
}
