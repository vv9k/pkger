use pkgspec::SpecStruct;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, SpecStruct)]
pub struct SourceDebControl {
    /// The name of the binary package.
    package: String,
    /// This field identifies the source package name
    source: String,
    /// The package maintainer’s name and email address. The name must come first, then the email address inside angle
    /// brackets <> (in RFC822 format).
    maintainer: String,
    /// The most recent version of the standards (the policy manual and associated texts) with which the package complies
    standards_version: String,
    /// Depending on context and the control file used, the Architecture field can include the following sets of values:
    ///  - A unique single word identifying a Debian machine architecture
    ///  - `any` matches all Debian machine architectures and is the most frequently used
    ///  - `all` which indicates an architecture-independent package
    ///  - `source` which indicates a source package
    architecture: String,
    /// Description of the package
    description: String,

    uploaders: Option<String>,
    /// This field specifies an application area into which the package has been classified
    section: Option<String>,
    /// This field represents how important it is that the user have the package installed
    priority: Option<String>,
    /// Estimate of the total amount of disk space required to install the named package
    installed_size: Option<String>,
    /// The URL of the web site for this package
    homepage: Option<String>,
    built_using: Option<String>,
    /// Simple field containing a word indicating the type of package:
    ///  - deb for binary packages
    ///  - udeb for micro binary packages
    /// Other types not defined here may be indicated.
    package_type: Option<String>,
    testsuite: Option<String>,
    rules_requires_root: Option<String>,

    // VCS
    arch: Option<String>,
    bzr: Option<String>,
    cvs: Option<String>,
    darcs: Option<String>,
    git: Option<String>,
    hg: Option<String>,
    mtn: Option<String>,
    svn: Option<String>,

    /// This is a boolean field which may occur only in the control file of a binary package or in a per-package fields
    /// paragraph of a source package control file.
    essential: bool,

    pre_depends: Vec<String>,
    depends: Vec<String>,
    recommends: Vec<String>,
    suggests: Vec<String>,
    breaks: Vec<String>,
    conflicts: Vec<String>,
    provides: Vec<String>,
    replaces: Vec<String>,
    enchances: Vec<String>,
}

impl SourceDebControl {
    pub fn save_to<P>(&self, path: P) -> io::Result<()>
    where
        P: AsRef<Path>,
    {
        fs::write(path, self.render())
    }

    pub fn render(&self) -> String {
        let mut control = format!(
            r#"Package:             {}
Source:              {}
Standards-Version:   {}
Architecture:        {}
Maintainer:          {}
Description:         {}
Essential:           {}
"#,
            &self.package,
            &self.source,
            &self.standards_version,
            &self.architecture,
            &self.maintainer,
            &self.description,
            if self.essential { "yes" } else { "no" }
        );

        macro_rules! if_some_push {
            ($field:ident, $fmt:expr) => {
                if let Some($field) = &self.$field {
                    control.push_str(&format!($fmt, $field));
                }
            };
        }

        macro_rules! if_not_empty_entries {
            ($field:ident, $fmt:expr) => {
                if !self.$field.is_empty() {
                    let last = self.$field.len() - 1;
                    let mut entries = String::new();
                    for (i, entry) in self.$field.iter().enumerate() {
                        entries.push_str(entry);
                        if i != last {
                            entries.push_str(", ");
                        }
                    }

                    control.push_str(&format!($fmt, entries));
                }
            };
        }

        #[rustfmt::skip]
        {
        if_some_push!(uploaders,           "Uploaders:           {}\n");
        if_some_push!(section,             "Section:             {}\n");
        if_some_push!(package_type,        "Package-Type:        {}\n");
        if_some_push!(testsuite,           "Testsuite:           {}\n");
        if_some_push!(priority,            "Priority:            {}\n");
        if_some_push!(installed_size,      "Installed-Size:      {}\n");
        if_some_push!(homepage,            "Homepage:            {}\n");
        if_some_push!(built_using,         "Built-Using:         {}\n");
        if_some_push!(rules_requires_root, "Rules-Requires-Root: {}\n");
        if_some_push!(arch,                "Arch:                {}\n");
        if_some_push!(bzr,                 "Bzr:                 {}\n");
        if_some_push!(cvs,                 "Cvs:                 {}\n");
        if_some_push!(darcs,               "Darcs:               {}\n");
        if_some_push!(git,                 "Git:                 {}\n");
        if_some_push!(hg,                  "Hg:                  {}\n");
        if_some_push!(mtn,                 "Mtn:                 {}\n");
        if_some_push!(svn,                 "Svn:                 {}\n");
        if_not_empty_entries!(pre_depends, "Pre-Depends:         {}\n");
        if_not_empty_entries!(depends,     "Depends:             {}\n");
        if_not_empty_entries!(recommends,  "Recommends:          {}\n");
        if_not_empty_entries!(suggests,    "Suggests:            {}\n");
        if_not_empty_entries!(breaks,      "Breaks:              {}\n");
        if_not_empty_entries!(conflicts,   "Conflicts:           {}\n");
        if_not_empty_entries!(provides,    "Provides:            {}\n");
        if_not_empty_entries!(replaces,    "Replaces:            {}\n");
        if_not_empty_entries!(enchances,   "Enchances:           {}\n");
        };

        control
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DebControlBuilder;

    #[test]
    fn builds_and_renders_source_control() {
        let expect = SourceDebControl {
            package: "debcontrol".to_string(),
            source: "package.tar.gz".to_string(),
            maintainer: "vv9k".to_string(),
            standards_version: "1".to_string(),
            architecture: "any".to_string(),
            description: "crate for DEB/control file generation".to_string(),
            uploaders: Some("wojciech@wkepka.dev".to_string()),
            section: Some("devel".to_string()),
            priority: None,
            installed_size: None,
            homepage: Some("https://github.com/vv9k/debcontrol".to_string()),
            built_using: Some("rustc".to_string()),
            package_type: Some("deb".to_string()),
            testsuite: None,
            rules_requires_root: Some("no".to_string()),
            arch: None,
            bzr: None,
            cvs: None,
            darcs: None,
            git: Some("https://github.com/vv9k/debcontrol/source.tar.gz".to_string()),
            hg: None,
            mtn: None,
            svn: None,
            essential: true,
            pre_depends: vec![],
            depends: vec!["rustc".to_string(), "cargo".to_string()],
            recommends: vec![],
            suggests: vec![],
            breaks: vec![],
            conflicts: vec![],
            provides: vec!["debcontrol".to_string()],
            replaces: vec![],
            enchances: vec![],
        };

        const OUT: &str = r#"Package:             debcontrol
Source:              package.tar.gz
Standards-Version:   1
Architecture:        any
Maintainer:          vv9k
Description:         crate for DEB/control file generation
Essential:           yes
Uploaders:           wojciech@wkepka.dev
Section:             devel
Package-Type:        deb
Homepage:            https://github.com/vv9k/debcontrol
Built-Using:         rustc
Rules-Requires-Root: no
Git:                 https://github.com/vv9k/debcontrol/source.tar.gz
Depends:             rustc, cargo
Provides:            debcontrol
"#;
        let got = DebControlBuilder::source_package_builder("debcontrol")
            .source("package.tar.gz")
            .standards_version("1")
            .architecture("any")
            .maintainer("vv9k")
            .description("crate for DEB/control file generation")
            .essential(true)
            .uploaders("wojciech@wkepka.dev")
            .section("devel")
            .package_type("deb")
            .homepage("https://github.com/vv9k/debcontrol")
            .built_using("rustc")
            .rules_requires_root("no")
            .git("https://github.com/vv9k/debcontrol/source.tar.gz")
            .add_depends_entries(vec!["rustc", "cargo"])
            .add_provides_entries(vec!["debcontrol"])
            .build();

        println!("{}", got.render());

        assert_eq!(expect, got);
        assert_eq!(OUT, got.render());
    }
}
