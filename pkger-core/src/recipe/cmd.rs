use crate::recipe::BuildTarget;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
/// Wrapper type for steps parsed from a recipe. Can be either a simple string or a map specifying
/// other parameters.
///
/// Examples:
/// "echo 123"
///
/// { cmd = "echo 123", images = ["centos8", "debian10"] }
///
/// { cmd = "echo 321", rpm = true } # execute only when building rpm target
pub struct Command {
    pub cmd: String,
    pub images: Option<Vec<String>>,
    pub rpm: Option<bool>,
    pub deb: Option<bool>,
    pub pkg: Option<bool>,
    pub gzip: Option<bool>,
    pub apk: Option<bool>,
}

impl From<&str> for Command {
    fn from(s: &str) -> Self {
        Self {
            cmd: s.to_string(),
            images: None,
            rpm: None,
            deb: None,
            pkg: None,
            gzip: None,
            apk: None,
        }
    }
}

impl Command {
    pub fn has_target_specified(&self) -> bool {
        self.rpm.is_some() || self.deb.is_some() || self.pkg.is_some() || self.gzip.is_some()
    }
    pub fn should_run_on(&self, target: &BuildTarget) -> bool {
        if !self.has_target_specified() {
            return true;
        }
        match &target {
            BuildTarget::Rpm => self.rpm,
            BuildTarget::Deb => self.deb,
            BuildTarget::Pkg => self.pkg,
            BuildTarget::Gzip => self.gzip,
            BuildTarget::Apk => self.apk,
        }
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn should_run_on_works() {
        let mut cmd = Command::from("echo 123");
        assert!(cmd.should_run_on(&BuildTarget::Deb));
        assert!(cmd.should_run_on(&BuildTarget::Rpm));
        assert!(cmd.should_run_on(&BuildTarget::Pkg));
        assert!(cmd.should_run_on(&BuildTarget::Gzip));
        assert!(cmd.should_run_on(&BuildTarget::Apk));
        cmd.rpm = Some(true);
        assert!(cmd.should_run_on(&BuildTarget::Rpm));
        assert!(!cmd.should_run_on(&BuildTarget::Gzip));
        assert!(!cmd.should_run_on(&BuildTarget::Pkg));
        assert!(!cmd.should_run_on(&BuildTarget::Deb));
        assert!(!cmd.should_run_on(&BuildTarget::Apk));
        cmd.deb = Some(true);
        cmd.pkg = Some(true);
        cmd.gzip = Some(true);
        cmd.apk = Some(true);
        assert!(cmd.should_run_on(&BuildTarget::Rpm));
        assert!(cmd.should_run_on(&BuildTarget::Gzip));
        assert!(cmd.should_run_on(&BuildTarget::Pkg));
        assert!(cmd.should_run_on(&BuildTarget::Deb));
        assert!(cmd.should_run_on(&BuildTarget::Apk));
    }
}
