use crate::recipe::{BuildArch, BuildTarget};
use crate::{ErrContext, Result};

use std::convert::TryFrom;
use std::fs::DirEntry;
use std::time::SystemTime;

#[derive(Debug, PartialEq)]
pub struct PackageMetadata {
    name: String,
    version: String,
    release: Option<String>,
    arch: Option<BuildArch>,
    package_type: BuildTarget,
    created: Option<SystemTime>,
}

impl PackageMetadata {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn release(&self) -> &Option<String> {
        &self.release
    }

    pub fn arch(&self) -> &Option<BuildArch> {
        &self.arch
    }

    pub fn package_type(&self) -> BuildTarget {
        self.package_type
    }

    pub fn created(&self) -> Option<SystemTime> {
        self.created
    }

    pub fn try_from_dir_entry(e: &DirEntry) -> Result<Self> {
        let path = e.path();
        let extension = path.extension().context("expected file extension")?;
        let package_type = BuildTarget::try_from(extension.to_string_lossy().as_ref())?;
        let path = path
            .file_stem()
            .context("expected a file name")?
            .to_string_lossy();
        let path = path.as_ref();

        Self::try_from_str(
            path,
            package_type,
            e.metadata().and_then(|md| md.created()).ok(),
        )
    }

    fn try_from_str(
        s: &str,
        package_type: BuildTarget,
        created: Option<SystemTime>,
    ) -> Result<Self> {
        match package_type {
            BuildTarget::Deb => {
                let mut elems = s.split('.').rev().peekable();
                let arch = elems.next().and_then(|s| BuildArch::try_from(s).ok());
                let patch = elems.next().context("expected patch number")?;
                let minor = elems.next().context("expected minor number")?;
                let (name, major) = elems
                    .next()
                    .map(|s| {
                        let mut elems = s.split('-').peekable();
                        let mut name = vec![];
                        while let Some(chunk) = elems.next() {
                            if elems.peek().is_some() {
                                name.push(chunk);
                            } else {
                                return (name.join("-"), chunk);
                            }
                        }

                        ("".to_string(), "")
                    })
                    .context("expected major number")?;
                let mut name_elems: Vec<_> = elems.collect();
                name_elems.push(name.as_str());
                let name = name_elems.join(".");

                Ok(PackageMetadata {
                    name,
                    version: format!("{}.{}.{}", major, minor, patch),
                    release: None,
                    arch,
                    package_type,
                    created,
                })
            }
            BuildTarget::Rpm => {
                let mut elems = s.split('.').rev().peekable();
                let arch = elems.next().and_then(|s| BuildArch::try_from(s).ok());
                let temp = elems
                    .next()
                    .map(|s| {
                        let mut _elems = s.split('-');
                        (
                            _elems.next().context("expected patch number"),
                            _elems.next().context("expected release number"),
                        )
                    })
                    .context("expected patch-release")?;
                let (patch, release) = (temp.0?, temp.1?);

                let minor = elems.next().context("expected minor number")?;
                let (name, major) = elems
                    .next()
                    .map(|s| {
                        let mut elems = s.split('-').peekable();
                        let mut name = vec![];
                        while let Some(chunk) = elems.next() {
                            if elems.peek().is_some() {
                                name.push(chunk);
                            } else {
                                return (name.join("-"), chunk);
                            }
                        }

                        ("".to_string(), "")
                    })
                    .context("expected major number")?;
                let mut name_elems: Vec<_> = elems.collect();
                name_elems.push(name.as_str());
                let name = name_elems.join(".");

                Ok(PackageMetadata {
                    name,
                    version: format!("{}.{}.{}", major, minor, patch),
                    release: Some(release.to_string()),
                    arch,
                    package_type,
                    created,
                })
            }
            BuildTarget::Pkg => {
                let mut elems = s.split('-').rev().peekable();
                let arch = elems.next().and_then(|s| BuildArch::try_from(s).ok());
                let release = elems.next().context("expected release number")?.to_string();
                let version = elems.next().context("expected version")?.to_string();
                let name = elems.rev().collect::<Vec<_>>().join("-");

                Ok(PackageMetadata {
                    name,
                    version,
                    release: Some(release),
                    arch,
                    package_type,
                    created,
                })
            }
            BuildTarget::Gzip => {
                let mut elems = s.split('-').rev().peekable();
                let version = elems.next().context("expected version")?.to_string();
                let name = elems.rev().collect::<Vec<_>>().join("-");

                Ok(PackageMetadata {
                    name,
                    version,
                    release: None,
                    arch: None,
                    package_type,
                    created,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::build::package::metadata::PackageMetadata;
    use crate::recipe::{BuildArch, BuildTarget};
    use std::time::SystemTime;

    #[test]
    fn parses_deb() {
        let path = "test-instantclient-19.10-basic-1.0.0.amd64";

        assert_eq!(
            PackageMetadata {
                name: "test-instantclient-19.10-basic".to_string(),
                version: "1.0.0".to_string(),
                release: None,
                arch: Some(BuildArch::x86_64),
                package_type: BuildTarget::Deb,
                created: None,
            },
            PackageMetadata::try_from_str(path, BuildTarget::Deb, None).unwrap(),
        );
    }

    #[test]
    fn parses_rpm() {
        let path = "tst-dev-tools-1.0.1-0.x86_64";

        let time = SystemTime::now();

        assert_eq!(
            PackageMetadata {
                name: "tst-dev-tools".to_string(),
                version: "1.0.1".to_string(),
                release: Some("0".to_string()),
                arch: Some(BuildArch::x86_64),
                package_type: BuildTarget::Rpm,
                created: Some(time),
            },
            PackageMetadata::try_from_str(path, BuildTarget::Rpm, Some(time)).unwrap(),
        );
    }

    #[test]
    fn parses_gzip() {
        let path = "tst-dev-tools-1.0.1";

        assert_eq!(
            PackageMetadata {
                name: "tst-dev-tools".to_string(),
                version: "1.0.1".to_string(),
                release: None,
                arch: None,
                package_type: BuildTarget::Gzip,
                created: None,
            },
            PackageMetadata::try_from_str(path, BuildTarget::Gzip, None).unwrap(),
        );
    }

    #[test]
    fn parses_pkg() {
        let path = "pkger-0.5.0-0-x86_64";

        assert_eq!(
            PackageMetadata {
                name: "pkger".to_string(),
                version: "0.5.0".to_string(),
                release: Some("0".to_string()),
                arch: Some(BuildArch::x86_64),
                package_type: BuildTarget::Pkg,
                created: None,
            },
            PackageMetadata::try_from_str(path, BuildTarget::Pkg, None).unwrap(),
        );
    }
}
