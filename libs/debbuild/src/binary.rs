use pkgspec::SpecStruct;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, SpecStruct)]
pub struct BinaryDebControl {
    /// The name of the binary package.
    package: String,
    /// The version number of a package. The format is: \[epoch:\]upstream_version\[-debian_revision\].
    version: String,
    /// Depending on context and the control file used, the Architecture field can include the following sets of values:
    ///  - A unique single word identifying a Debian machine architecture
    ///  - `any` matches all Debian machine architectures and is the most frequently used
    ///  - `all` which indicates an architecture-independent package
    ///  - `source` which indicates a source package
    architecture: String,
    /// Description of the package
    description: String,

    /// This part of the version number specifies the version of the Debian package based on the upstream version
    revision: Option<String>,
    /// Epochs can help when the upstream version numbering scheme changes, but they must be used with care
    epoch: Option<String>,
    /// The package maintainer’s name and email address. The name must come first, then the email address inside angle
    /// brackets <> (in RFC822 format)
    maintainer: Option<String>,
    /// This field identifies the source package name
    source: Option<String>,
    /// This field specifies an application area into which the package has been classified
    section: Option<String>,
    /// This field represents how important it is that the user have the package installed
    priority: Option<String>,
    /// Estimate of the total amount of disk space required to install the named package
    installed_size: Option<String>,
    /// The URL of the web site for this package
    homepage: Option<String>,
    built_using: Option<String>,

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

impl BinaryDebControl {
    pub fn save_to<P>(&self, path: P) -> io::Result<()>
    where
        P: AsRef<Path>,
    {
        fs::write(path, self.render())
    }

    pub fn render(&self) -> String {
        let revision = if let Some(revision) = &self.revision {
            revision
        } else {
            "0"
        };
        let version = if let Some(epoch) = &self.epoch {
            format!("{}:{}-{}", epoch, &self.version, revision)
        } else {
            format!("{}-{}", &self.version, revision)
        };

        let mut control = format!(
            r#"Package:        {}
Version:        {}
Architecture:   {}
Description:    {}
Essential:      {}
"#,
            &self.package,
            version,
            &self.architecture,
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
        if_some_push!(maintainer,          "Maintainer:     {}\n");
        if_some_push!(source,              "Source:         {}\n");
        if_some_push!(section,             "Section:        {}\n");
        if_some_push!(priority,            "Priority:       {}\n");
        if_some_push!(installed_size,      "Installed-Size: {}\n");
        if_some_push!(homepage,            "Homepage:       {}\n");
        if_some_push!(built_using,         "Built-Using:    {}\n");
        if_not_empty_entries!(pre_depends, "Pre-Depends:    {}\n");
        if_not_empty_entries!(depends,     "Depends:        {}\n");
        if_not_empty_entries!(recommends,  "Recommends:     {}\n");
        if_not_empty_entries!(suggests,    "Suggests:       {}\n");
        if_not_empty_entries!(breaks,      "Breaks:         {}\n");
        if_not_empty_entries!(conflicts,   "Conflicts:      {}\n");
        if_not_empty_entries!(provides,    "Provides:       {}\n");
        if_not_empty_entries!(replaces,    "Replaces:       {}\n");
        if_not_empty_entries!(enchances,   "Enchances:      {}\n");
        };

        control
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DebControlBuilder;

    #[test]
    fn builds_and_renders_binary_control() {
        let expect = BinaryDebControl {
            package: "debcontrol".to_string(),
            version: "1.0.0".to_string(),
            revision: None,
            epoch: None,
            architecture: "any".to_string(),
            maintainer: Some("vv9k".to_string()),
            description: "crate for DEB/control file generation".to_string(),
            source: Some("package.tar.gz".to_string()),
            section: Some("devel".to_string()),
            priority: None,
            installed_size: Some("1Mb".to_string()),
            homepage: Some("https://some.invalid.url".to_string()),
            built_using: Some("rustc".to_string()),
            essential: true,
            pre_depends: vec!["rustc".to_string(), "cargo".to_string()],
            depends: vec!["rustc".to_string(), "cargo".to_string()],
            recommends: vec![],
            suggests: vec![],
            breaks: vec![],
            conflicts: vec!["rustc".to_string(), "cargo".to_string()],
            provides: vec![
                "rustc".to_string(),
                "cargo".to_string(),
                "debcontrol".to_string(),
            ],
            replaces: vec!["rustc".to_string(), "cargo".to_string()],
            enchances: vec!["rustc".to_string(), "cargo".to_string()],
        };
        const OUT: &str = r#"Package:        debcontrol
Version:        1.0.0-0
Architecture:   any
Description:    crate for DEB/control file generation
Essential:      yes
Maintainer:     vv9k
Source:         package.tar.gz
Section:        devel
Installed-Size: 1Mb
Homepage:       https://some.invalid.url
Built-Using:    rustc
Pre-Depends:    rustc, cargo
Depends:        rustc, cargo
Conflicts:      rustc, cargo
Provides:       rustc, cargo, debcontrol
Replaces:       rustc, cargo
Enchances:      rustc, cargo
"#;
        let got = DebControlBuilder::binary_package_builder("debcontrol")
            .source("package.tar.gz")
            .version("1.0.0")
            .architecture("any")
            .maintainer("vv9k")
            .description("crate for DEB/control file generation")
            .essential(true)
            .installed_size("1Mb")
            .section("devel")
            .homepage("https://some.invalid.url")
            .built_using("rustc")
            .add_pre_depends_entries(vec!["rustc", "cargo"])
            .add_depends_entries(vec!["rustc", "cargo"])
            .add_conflicts_entries(vec!["rustc", "cargo"])
            .add_provides_entries(vec!["rustc", "cargo"])
            .add_replaces_entries(vec!["rustc", "cargo"])
            .add_enchances_entries(vec!["rustc", "cargo"])
            .add_provides_entries(vec!["debcontrol"])
            .build();
        println!("{}", got.render());

        assert_eq!(expect, got);
        assert_eq!(OUT, got.render());
    }

    #[test]
    fn constructs_correct_version() {
        static EXPECT: &str = r#"Package:        test
Version:        42:1.0.0-5
Architecture:   
Description:    
Essential:      no
"#;

        let got = DebControlBuilder::binary_package_builder("test")
            .version("1.0.0")
            .revision("5")
            .epoch("42")
            .build()
            .render();
        assert_eq!(EXPECT, got);
    }
}
