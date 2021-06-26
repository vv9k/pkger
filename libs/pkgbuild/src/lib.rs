use pkgspec::SpecStruct;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, SpecStruct)]
pub struct PkgBuild {
    #[skip]
    /// Name of this packages or names if split packages
    pkgname: Vec<String>,
    /// The version of the software. The variable is not allowed to contain colons,
    /// forward slashes, hyphens or whitespace
    pkgver: String,
    /// Release number of the package
    pkgrel: String,
    /// Architectures on which the given package is available
    arch: Vec<String>,
    /// A brief description of the package
    pkgdesc: Option<String>,
    /// Used to force the package to be seen as newer. Use as a last resort
    epoch: Option<String>,
    /// The url pointing to the website of the package
    url: Option<String>,
    /// License(s) of the package
    license: Vec<String>,
    /// Specifies a special install script that is to be included in the package
    install: Option<String>,
    /// Specifies a changelog file that is to be included in the package
    changelog: Option<String>,
    /// A list of source files required to build the package
    source: Vec<String>,
    /// A list of PGP fingerprints
    validpgpkeys: Vec<String>,
    /// An array of file names corresponding to those from the source array. Files listed
    /// here will not be extracted with the rest of the source files
    noextract: Vec<String>,

    /// A list of MD5 hashes for every source file specified in `source` list
    md5sums: Vec<String>,
    /// A list of SHA1 hashes for every source file specified in `source` list
    sha1sums: Vec<String>,
    /// A list of SHA224 hashes for every source file specified in `source` list
    sha224sums: Vec<String>,
    /// A list of SHA256 hashes for every source file specified in `source` list
    sha256sums: Vec<String>,
    /// A list of SHA384 hashes for every source file specified in `source` list
    sha384sums: Vec<String>,
    /// A list of SHA512 hashes for every source file specified in `source` list
    sha512sums: Vec<String>,
    /// A list of BLAKE2 hashes for every source file specified in `source` list
    b2sums: Vec<String>,

    /// A list of symbolic names that represent groups of packages
    groups: Vec<String>,
    /// A list of file names (paths must be relative) that should be backed up if the package
    /// is removed or upgraded
    backup: Vec<String>,

    /// A list of packages this package depends on to run
    depends: Vec<String>,
    /// A list of packages this package depends on to build
    makedepends: Vec<String>,
    /// A list of packages this package depends on to run it's test suite
    checkdepends: Vec<String>,
    /// A list of packages that are not essential for base functionality, but may be necessary to
    /// make full use of the package
    optdepends: Vec<String>,
    /// A list of packages that will conflict with this package
    conflicts: Vec<String>,
    /// A list of "virtual provisions" that this package provides
    provides: Vec<String>,
    /// A list of packages this package replaces
    replaces: Vec<String>,

    #[skip]
    options: Vec<String>,

    /// The function that is used to install files into the directory that will become the root
    /// directory of the build package
    package_func: String,
    /// An optional function that prepares the sources for the building
    prepare_func: Option<String>,
    /// An optional function used to compile and adjust source files in preparation for install
    build_func: Option<String>,
    /// An optional function that should test the functionality of the package before installation
    check_func: Option<String>,
}

impl PkgBuild {
    /// Renders this PKGBUILD and saves it to the given path
    pub fn save_to<P>(&self, path: P) -> io::Result<()>
    where
        P: AsRef<Path>,
    {
        fs::write(path, self.render())
    }

    /// Renders this PKGBUILD
    pub fn render(&self) -> String {
        let mut pkg = String::new();

        macro_rules! push_field {
            ($field:ident) => {
                pkg.push_str(&format!("{}={}\n", stringify!($field), &self.$field));
            };
        }

        macro_rules! push_if_some {
            ($field:ident) => {
                if let Some(value) = &self.$field {
                    pkg.push_str(&format!("{}={}\n", stringify!($field), value));
                }
            };
        }

        macro_rules! push_array {
            ($field:ident) => {
                if !self.$field.is_empty() {
                    let elems: Vec<_> = self
                        .$field
                        .iter()
                        .map(|elem| format!("'{}'", elem))
                        .collect();
                    pkg.push_str(&format!("{}=({})\n", stringify!($field), elems.join(" ")));
                }
            };
        }

        macro_rules! push_func {
            ($field:ident) => {
                pkg.push_str(&format!("\n{}() {{\n{}\n}}\n", stringify!($field), $field));
            };
        }

        push_array!(pkgname);
        push_field!(pkgver);
        push_field!(pkgrel);
        push_array!(arch);
        if let Some(value) = &self.pkgdesc {
            pkg.push_str(&format!("pkgdesc='{}'\n", value));
        }
        push_if_some!(epoch);
        push_if_some!(url);
        push_array!(license);
        push_if_some!(install);
        push_if_some!(changelog);
        push_array!(source);
        push_array!(validpgpkeys);
        push_array!(noextract);
        push_array!(md5sums);
        push_array!(sha1sums);
        push_array!(sha224sums);
        push_array!(sha256sums);
        push_array!(sha384sums);
        push_array!(sha512sums);
        push_array!(b2sums);
        push_array!(groups);
        push_array!(backup);
        push_array!(depends);
        push_array!(makedepends);
        push_array!(checkdepends);
        push_array!(optdepends);
        push_array!(conflicts);
        push_array!(provides);
        push_array!(replaces);
        push_array!(options);

        let package = &self.package_func;
        push_func!(package);
        if let Some(prepare) = &self.prepare_func {
            push_func!(prepare);
        }
        if let Some(build) = &self.build_func {
            push_func!(build);
        }
        if let Some(check) = &self.check_func {
            push_func!(check);
        }

        pkg
    }
}

impl PkgBuildBuilder {
    /// The name of the package
    pub fn pkgname<S>(mut self, name: S) -> Self
    where
        S: Into<String>,
    {
        self.inner.pkgname.push(name.into());
        self
    }

    /// The names of the packages for split packages
    pub fn pkgnames<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        names
            .into_iter()
            .for_each(|name| self.inner.pkgname.push(name.into()));
        self
    }

    /// Add option to strip symbols from binaries and libraries
    pub fn opt_strip(mut self) -> Self {
        self.inner.options.push("strip".to_string());
        self
    }

    /// Add option to save doc directories
    pub fn opt_docs(mut self) -> Self {
        self.inner.options.push("docs".to_string());
        self
    }

    /// Add option to leave libtool (.la) files in packages
    pub fn opt_libtool(mut self) -> Self {
        self.inner.options.push("libtool".to_string());
        self
    }

    /// Add option that adds debug flags to buildflags
    pub fn opt_debug(mut self) -> Self {
        self.inner.options.push("debug".to_string());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn builds_a_pkgbuild() {
        let got = PkgBuild::builder()
            .pkgname("pkgbuild")
            .pkgver("0.1.0")
            .pkgrel("1")
            .epoch("42")
            .pkgdesc("short description...")
            .url("https://github.com/wojciechkepka/pkgbuild")
            .add_license_entries(vec!["MIT"])
            .install("install.sh")
            .changelog("CHANGELOG.md")
            .add_source_entries(vec!["src1.tar.gz", "src2.tar.gz", "src3.tar.gz"])
            .add_depends_entries(vec!["rust", "cargo"])
            .add_provides_entries(vec!["pkgbuild-rs"])
            .build_func("    echo test")
            .check_func("    true\n    false")
            .build()
            .render();

        let expect = r#"pkgname=('pkgbuild')
pkgver=0.1.0
pkgrel=1
pkgdesc='short description...'
epoch=42
url=https://github.com/wojciechkepka/pkgbuild
license=('MIT')
install=install.sh
changelog=CHANGELOG.md
source=('src1.tar.gz' 'src2.tar.gz' 'src3.tar.gz')
depends=('rust' 'cargo')
provides=('pkgbuild-rs')

package() {

}

build() {
    echo test
}

check() {
    true
    false
}
"#;

        assert_eq!(expect, got);
    }
}
