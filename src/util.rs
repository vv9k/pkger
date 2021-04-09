//! Helper functions that don't fit anywhere else

use std::path::{Path, PathBuf};

#[macro_export]
macro_rules! map_return {
    ($f:expr, $e:expr) => {
        match $f {
            Ok(d) => d,
            Err(e) => return Err(anyhow!("{} - {}", $e, e)),
        }
    };
}

pub fn find_penultimate_ancestor<P: AsRef<Path>>(path: P) -> PathBuf {
    let mut ancestors = path.as_ref().ancestors();
    loop {
        match ancestors.next() {
            Some(ancestor) => {
                if ancestors.next() == Some(Path::new("")) {
                    return ancestor.to_path_buf();
                }
            }
            None => return PathBuf::from(""),
        }
    }
}

pub fn should_include<P: AsRef<Path>>(path: P, excludes: &[String]) -> bool {
    for e in excludes {
        if path.as_ref().starts_with(e) {
            return false;
        }
    }
    true
}
