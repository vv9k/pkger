use pkgspec::SpecStruct;
use pkgspec_core::{Error, Manifest, Result};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq, SpecStruct)]
pub struct ApkBuild {
    #[skip]
    /// Name of this packages or names if split packages
    pkgname: String,
    /// The version of the software. The variable is not allowed to contain colons,
    /// forward slashes, hyphens or whitespace
    pkgver: String,
    /// Release number of the package
    pkgrel: String,
    /// Architectures on which the given package is available
    arch: Vec<String>,
    /// A brief description of the package
    pkgdesc: String,
    /// The url pointing to the website of the package
    url: String,
    /// License(s) of the package
    license: Vec<String>,
    /// A list of source files required to build the package
    source: Vec<String>,
    /// If the package contains pre/post install scripts this field should contain the install
    /// variables.
    install: Option<String>,

    /// Subpackages are made to split up the normal "make install" into separate packages. The most
    /// common subpackages we use are doc and dev
    subpackages: Vec<String>,

    /// A list of patches to apply to the package
    patches: Vec<String>,

    /// A list of "virtual provisions" that this package provides
    provides: Vec<String>,
    /// A list of packages this package depends on to run
    depends: Vec<String>,
    /// A list of packages this package depends on to build
    makedepends: Vec<String>,

    /// Directory used during prepare/build/install phases
    builddir: String,

    /// Specifies the prepare script of the package
    prepare_func: Option<String>,
    /// Specifies the check script of the package
    check_func: Option<String>,
    /// Specifies the build script of the package
    build_func: Option<String>,
    /// Specifies the package script of the package
    package_func: Option<String>,
}

impl Manifest for ApkBuild {
    /// Renders this APKBUILD and saves it to the given path
    fn save_to(&self, path: impl AsRef<Path>) -> Result<()> {
        fs::write(path, self.render()?).map_err(Error::from)
    }

    /// Renders this APKBUILD
    fn render(&self) -> Result<String> {
        use std::fmt::Write;
        let mut pkg = String::new();

        macro_rules! format_value {
            ($key:expr, $value:ident) => {
                if $value.contains(|c: char| c.is_ascii_whitespace() || c == '$') {
                    write!(pkg, "{}=\"{}\"\n", $key, &$value)?;
                } else {
                    write!(pkg, "{}={}\n", $key, &$value)?;
                }
            };
        }

        macro_rules! push_field {
            ($field:ident) => {
                let f = &self.$field;
                format_value!(stringify!($field), f);
            };
        }

        macro_rules! push_if_some {
            ($field:ident) => {
                if let Some(value) = &self.$field {
                    format_value!(stringify!($field), value);
                }
            };
        }

        macro_rules! push_array {
            ($field:ident) => {
                if !self.$field.is_empty() {
                    write!(
                        pkg,
                        "{}=\"{}\"\n",
                        stringify!($field),
                        self.$field.join(" ")
                    )?;
                }
            };
        }

        macro_rules! push_func {
            ($field:ident) => {
                write!(pkg, "\n{}() {{\n{}\n}}\n", stringify!($field), $field)?;
            };
        }

        push_field!(pkgname);
        push_field!(pkgver);
        push_field!(pkgrel);
        push_field!(pkgdesc);
        push_field!(url);
        push_array!(arch);
        push_array!(license);
        push_array!(provides);
        push_array!(depends);
        push_array!(makedepends);
        push_if_some!(install);
        push_array!(subpackages);
        push_array!(source);
        push_array!(patches);
        if self.builddir.is_empty() {
            const BUILDDIR: &str = "$srcdir/";
            format_value!("builddir", BUILDDIR);
        } else {
            push_field!(builddir);
        }

        if let Some(prepare) = &self.prepare_func {
            push_func!(prepare);
        }
        if let Some(build) = &self.build_func {
            push_func!(build);
        }
        if let Some(check) = &self.check_func {
            push_func!(check);
        }
        if let Some(package) = &self.package_func {
            push_func!(package);
        }

        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_apkbuild() {
        let got = ApkBuild::builder()
            .pkgname("apkbuild")
            .pkgver("0.1.0")
            .pkgrel("1")
            .pkgdesc("short description...")
            .url("https://some.invalid.url")
            .add_license_entries(vec!["MIT"])
            .add_depends_entries(vec!["rust", "cargo"])
            .build_func("    echo test")
            .check_func("    true\n    false")
            .build()
            .render();

        let expect = r#"pkgname=apkbuild
pkgver=0.1.0
pkgrel=1
pkgdesc="short description..."
url=https://some.invalid.url
license="MIT"
depends="rust cargo"
builddir="$srcdir/"

build() {
    echo test
}

check() {
    true
    false
}
"#;

        assert_eq!(expect, got.unwrap());
    }
}
