use pkger_core::recipe::{BuildArch, BuildTarget};
use pkger_core::{ErrContext, Result};

use lazy_static::lazy_static;
use regex::Regex;
use std::convert::TryFrom;
use std::fs::{DirEntry, Metadata};
use std::time::SystemTime;

lazy_static! {
    static ref DEB_RE: Regex = Regex::new(r"([\w.+-]+?)-([\d.]+)-(\d+)[.]([\w_-]+)").unwrap();
    static ref RPM_RE: Regex = Regex::new(r"([\w_.+-]+?)-([\d.]+)-(\d+)[.]([\w_-]+)").unwrap();
    static ref PKG_RE: Regex = Regex::new(r"([\w_.+@-]+?)-([\d.]+)-(\d+)-([\w_-]+)").unwrap();
    static ref GZIP_RE: Regex = Regex::new(r"([\S]+?)-(\d+[.]\d+[.]\d+)").unwrap();
    static ref APK_RE: Regex = Regex::new(r"([\w_.+@-]+?)-(\d+[.]\d+[.]\d+)-r(\d+)").unwrap();
}

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "deb", "src.deb", "rpm", "src.rpm", "srpm", "pkg", "apk", "gzip", "tar.gz", "tgz",
];

#[cfg(unix)]
fn size(md: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    md.size()
}

#[cfg(windows)]
fn size(md: &Metadata) -> u64 {
    use std::os::windows::fs::MetadataExt;
    md.file_size()
}

#[derive(Debug, PartialEq, Eq)]
pub struct PackageMetadata {
    name: String,
    version: String,
    release: Option<String>,
    arch: Option<BuildArch>,
    package_type: BuildTarget,
    created: Option<SystemTime>,
    size: Option<u64>, // in bytes
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

    #[allow(dead_code)]
    pub fn size(&self) -> Option<u64> {
        self.size
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

        let (created, size) = e
            .metadata()
            .map(|md| (md.created().ok(), Some(size(&md))))
            .ok()
            .unwrap_or((None, None));

        Self::try_from_str(path, package_type, created, size)
            .context("invalid package name, the name did not match any scheme")
    }

    fn try_from_str(
        s: &str,
        package_type: BuildTarget,
        created: Option<SystemTime>,
        size: Option<u64>,
    ) -> Option<Self> {
        match package_type {
            BuildTarget::Deb => DEB_RE
                .captures_iter(s)
                .next()
                .map(|captures| PackageMetadata {
                    name: captures[1].to_string(),
                    version: captures[2].to_string(),
                    release: Some(captures[3].to_string()),
                    arch: BuildArch::try_from(&captures[4]).ok(),
                    package_type,
                    created,
                    size,
                }),
            BuildTarget::Rpm => RPM_RE
                .captures_iter(s)
                .next()
                .map(|captures| PackageMetadata {
                    name: captures[1].to_string(),
                    version: captures[2].to_string(),
                    release: Some(captures[3].to_string()),
                    arch: BuildArch::try_from(&captures[4]).ok(),
                    package_type,
                    created,
                    size,
                }),
            BuildTarget::Pkg => PKG_RE
                .captures_iter(s)
                .next()
                .map(|captures| PackageMetadata {
                    name: captures[1].to_string(),
                    version: captures[2].to_string(),
                    release: Some(captures[3].to_string()),
                    arch: BuildArch::try_from(&captures[4]).ok(),
                    package_type,
                    created,
                    size,
                }),
            BuildTarget::Gzip => GZIP_RE
                .captures_iter(s)
                .next()
                .map(|captures| PackageMetadata {
                    name: captures[1].to_string(),
                    version: captures[2].to_string(),
                    release: None,
                    arch: None,
                    package_type,
                    created,
                    size,
                }),
            BuildTarget::Apk => APK_RE
                .captures_iter(s)
                .next()
                .map(|captures| PackageMetadata {
                    name: captures[1].to_string(),
                    version: captures[2].to_string(),
                    release: Some(captures[3].to_string()),
                    arch: None,
                    package_type,
                    created,
                    size,
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PackageMetadata;
    use pkger_core::recipe::{BuildArch, BuildTarget};
    use std::time::SystemTime;

    #[test]
    fn parses_deb() {
        let path = "test-instantclient-19.10-basic-1.0.0-1.amd64";

        assert_eq!(
            PackageMetadata {
                name: "test-instantclient-19.10-basic".to_string(),
                version: "1.0.0".to_string(),
                release: Some(1.to_string()),
                arch: Some(BuildArch::x86_64),
                package_type: BuildTarget::Deb,
                created: None,
                size: None,
            },
            PackageMetadata::try_from_str(path, BuildTarget::Deb, None, None).unwrap(),
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
                size: None,
            },
            PackageMetadata::try_from_str(path, BuildTarget::Rpm, Some(time), None).unwrap(),
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
                size: None,
            },
            PackageMetadata::try_from_str(path, BuildTarget::Gzip, None, None).unwrap(),
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
                size: None,
            },
            PackageMetadata::try_from_str(path, BuildTarget::Pkg, None, None).unwrap(),
        );
    }
}
