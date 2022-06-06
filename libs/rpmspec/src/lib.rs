use pkgspec::SpecStruct;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, SpecStruct)]
pub struct RpmSpec {
    /// The base name of the package, which should match the SPEC filename.
    name: String,
    /// The upstream version number of the software.
    version: String,
    /// The number of times this version of the software was released. Normally, set the initial value to 1%{?dist},
    /// and increment it with each new release of the package. Reset to 1 when a new Version of the software is built.
    release: String,
    /// A way to define weighted dependencies based on version numbers.
    epoch: Option<String>,
    vendor: Option<String>,
    /// The full URL for more information about the program. Most often this is the upstream project website for the
    /// software being packaged.
    url: Option<String>,
    /// Copyright notice
    copyright: Option<String>,
    /// A person maintaining this package
    packager: Option<String>,
    /// Helps users categorize this package
    group: Option<String>,
    /// A desktop icon for this package
    icon: Option<String>,
    /// A brief, one-line summary of the package.
    summary: Option<String>,
    /// The license of the software being packaged
    license: Option<String>,
    build_root: Option<String>,

    /// Paths or URLs to the compressed archives of the upstream source code(unpatched, patches are handled elsewhere).
    /// This should point to an accessible and reliable storage of the archive, for example, the upstream page and not
    /// the packager’s local storage.
    sources: Vec<String>,
    /// Names of patches to apply to the source code if necessary
    patches: Vec<String>,
    /// If the package is not architecture dependent, for example, if written entirely in an interpreted programming
    /// language, set this to `noarch`. If not set, the package automatically inherits the Architecture of the machine
    /// on which it is built, for example `x86_64`
    build_arch: Option<String>,
    /// If a piece of software can not operate on a specific processorarchitecture, you can exclude that architecture
    /// here.
    exclude_arch: Option<String>,

    // dependencies
    /// Where one package conflicts with a capability provided by another
    conflicts: Vec<String>,
    /// Where one package obsoletes capabilities provided by another, usually used when a package changes name and
    /// the new package obsoletes the old name.
    obsoletes: Vec<String>,
    /// A listing of the capabilities this package provides
    provides: Vec<String>,
    /// Packages that are required by this package at runtime
    requires: Vec<String>,
    /// Packages that are required by this package during the build
    build_requires: Vec<String>,

    description: String,

    /// Command or series of commands to prepare the software to be built, for example, unpacking the archive in Source0.
    /// This directive can contain a shell script.
    prep_script: Option<String>,
    /// Command or series of commands to test the software. This normally includes things such as unit tests.
    check_script: Option<String>,
    /// Command or series of commands for actually building the software into machine code (for compiled languages) or
    /// byte code (for some interpreted languages).
    build_script: Option<String>,
    /// Command or series of commands for copying the desired build artifacts from the %builddir (where the build happens)
    /// to the %buildroot directory.
    install_script: Option<String>,

    /// Scriptlet that is executed just before the package is installed on the targetsystem.
    pre_script: Option<String>,
    /// Scriptlet that is executed just after the package is installed on the targetsystem.
    post_script: Option<String>,
    /// Scriptlet that is executed just before the package is uninstalled from the targetsystem.
    preun_script: Option<String>,
    /// Scriptlet that is executed just after the package is uninstalled from the targetsystem.
    postun_script: Option<String>,

    files: Vec<String>,
    /// This identifies the file listed as documentation and it will be installed and labeled as such by RPM. This is
    /// often used not only for documentation about the software being packaged but also code examples and various items
    /// that should accompany documentation. In the event code examples are included, care should be taken to remove
    /// executable mode from the file.
    doc_files: Vec<String>,
    /// This identifies the file listed as a LICENSE file and it will be installed and labeled as such by RPM.
    license_files: Vec<String>,
    /// Identifies that the path is a directory that should be owned by this RPM. This is important so that the RPM file
    /// manifest accurately knows what directories to clean up on uninstall.
    ///
    /// Example: `%{_libdir}/%{name}`
    dir_files: Vec<String>,
    /// Specifies that the following file is a configuration file and therefore should not be overwritten (or replaced)
    /// on a package install or update if the file has been modified from the original installation checksum. In the event
    /// that there is a change, the file will be created with .rpmnew appended to the end of the filename upon upgrade or
    /// install so that the pre-existing or modified file on the target system is not modified.
    ///
    /// Example: `%{_sysconfdir}/%{name}/%{name}.conf`
    config_noreplace: Option<String>,

    changelog: Vec<String>,

    #[skip]
    /// User defined macros
    macros: Vec<String>,

    #[skip]
    /// Set the value of `AutoReqProv` field in the spec. If set to `false` RPM won't do automatic
    /// dependencies processing.
    auto_req_prov: Option<bool>,
}

impl RpmSpec {
    pub fn save_to<P>(&self, path: P) -> io::Result<()>
    where
        P: AsRef<Path>,
    {
        fs::write(path, self.render())
    }

    pub fn render(&self) -> String {
        let summary = if let Some(summary) = &self.summary {
            summary.as_str()
        } else {
            "missing"
        };
        let mut spec = format!(
            "Name:          {}\nVersion:       {}\nRelease:       {}\nSummary:       {}\n",
            self.name, self.version, self.release, summary
        );
        macro_rules! if_some_push {
            ($field:ident, $fmt:expr) => {
                if let Some($field) = &self.$field {
                    spec.push_str(&format!($fmt, $field));
                }
            };
        }

        macro_rules! if_not_empty_entries {
            ($field:ident, $fmt:expr) => {
                if !self.$field.is_empty() {
                    for entry in self.$field.iter() {
                        spec.push_str(&format!($fmt, entry));
                    }
                }
            };
            (..i $field:ident, $fmt:expr) => {
                if !self.$field.is_empty() {
                    for (i, entry) in self.$field.iter().enumerate() {
                        spec.push_str(&format!($fmt, i, entry));
                    }
                }
            };
            (file $field:ident, $name:expr) => {
                if !self.$field.is_empty() {
                    spec.push_str(&format!("\n%{}\n", $name));
                    for entry in &self.$field {
                        spec.push('"');
                        spec.push_str(entry.as_str());
                        spec.push_str("\"\n");
                    }
                }
            };
        }

        macro_rules! if_some_script {
            ($script:expr, $field:ident) => {
                spec.push_str(&format!("%{}\n", $script));
                if let Some(script) = &self.$field {
                    spec.push_str(script);
                    spec.push_str("\n\n");
                }
            };
        }

        #[rustfmt::skip]
        {
        if_some_push!(epoch,                  "Epoch:         {}\n");
        if_some_push!(vendor,                 "Vendor:        {}\n");
        if_some_push!(url,                    "URL:           {}\n");
        if_some_push!(copyright,              "Copyright:     {}\n");
        if_some_push!(packager,               "Packager:      {}\n");
        if_some_push!(group,                  "Group:         {}\n");
        if_some_push!(icon,                   "Icon:          {}\n");
        if_some_push!(license,                "License:       {}\n");
        if_some_push!(build_root,             "BuildRoot:     {}\n");
        if_some_push!(build_arch,             "BuildArch:     {}\n");
        if_some_push!(exclude_arch,           "ExcludeArch:   {}\n");
        if_not_empty_entries!(conflicts,      "Conflicts:     {}\n");
        if_not_empty_entries!(obsoletes,      "obsoletes:     {}\n");
        if_not_empty_entries!(provides,       "provides:      {}\n");
        if_not_empty_entries!(requires,       "requires:      {}\n");
        if_not_empty_entries!(build_requires, "BuildRequires: {}\n");
        if_not_empty_entries!(..i patches,    "Patch{}:        {}\n");
        if_not_empty_entries!(..i sources,    "Source{}:       {}\n");
        if let Some(auto_req_prov) = self.auto_req_prov {
            spec.push_str("AutoReqProv:   ");
            let enable = if auto_req_prov {"Yes"} else {"No"};
            spec.push_str(enable);
            spec.push('\n');
        }
        spec.push_str(&format!("\n%description\n{}\n\n", self.description));
        if_some_script!("prep", prep_script);
        if_some_script!("build", build_script);
        if_some_script!("install", install_script);
        if_some_script!("check", check_script);
        if_some_script!("pre", pre_script);
        if_some_script!("post", post_script);
        if_some_script!("preun", preun_script);
        if_some_script!("postun", postun_script);
        if_not_empty_entries!(macros, "%global {}\n");
        };
        spec.push_str("\n%files\n");
        for entry in &self.files {
            spec.push('"');
            spec.push_str(entry.as_str());
            spec.push_str("\"\n");
        }
        if_not_empty_entries!(file doc_files, "doc");
        if_not_empty_entries!(file license_files, "license");
        if_not_empty_entries!(file dir_files, "dir");
        spec.push_str("\n%changelog\n");
        for entry in &self.changelog {
            spec.push_str(entry.as_str());
            spec.push('\n');
        }

        spec
    }
}

impl RpmSpecBuilder {
    pub fn add_macro<N, O, B>(mut self, name: N, opts: Option<O>, body: B) -> Self
    where
        N: AsRef<str>,
        O: AsRef<str>,
        B: AsRef<str>,
    {
        let _macro = if let Some(opts) = opts {
            format!("{}({}) {}", name.as_ref(), opts.as_ref(), body.as_ref())
        } else {
            format!("{} {}", name.as_ref(), body.as_ref())
        };
        self.inner.macros.push(_macro);
        self
    }

    pub fn disable_auto_req_prov(mut self) -> Self {
        self.inner.auto_req_prov = Some(false);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_renders_a_spec() {
        const BUILD: &str = r#"echo 123 > test.bin
echo 321 > README"#;
        const INSTALL: &str = r#"install -m 755 test.bin /bin/test.bin
install -m 644 README /docs/README"#;

        let spec = RpmSpec::builder()
            .name("rpmspec")
            .license("MIT")
            .summary("short summary")
            .description("very long summary...")
            .build_script(BUILD)
            .install_script(INSTALL)
            .prep_script(r#"cat /etc/os-release"#)
            .check_script("uptime")
            .url("https://some.invalid.url")
            .version("0.1.0")
            .release("1")
            .epoch("42")
            .vendor("Vendor")
            .packager("vv9k")
            .copyright("2021 test")
            .build_arch("noarch")
            .exclude_arch("x86_64")
            .group("group")
            .icon("rpm.xpm")
            .build_root("/root/bld")
            .add_patches_entries(vec!["patch.1", "patch.2"])
            .add_sources_entries(vec!["source.tar.gz", "source-2.tar.xz"])
            .add_files_entries(vec!["/bin/test.bin", "/docs/README"])
            .add_doc_files_entries(vec!["README"])
            .add_license_files_entries(vec!["LICENSE"])
            .add_provides_entries(vec!["rpmspec"])
            .add_requires_entries(vec!["rust"])
            .add_build_requires_entries(vec!["rust", "cargo"])
            .add_obsoletes_entries(vec!["rpmspec-old"])
            .add_conflicts_entries(vec!["rpmspec2"])
            .config_noreplace("%{_sysconfdir}/%{name}/%{name}.conf")
            .pre_script("echo")
            .post_script("false")
            .preun_script("echo 123")
            .postun_script("true")
            .add_macro("githash", None::<&str>, "0ab32f")
            .add_macro("python", Some("-c"), "import os")
            .disable_auto_req_prov()
            .build();

        let expect = RpmSpec {
            name: "rpmspec".to_string(),
            version: "0.1.0".to_string(),
            release: "1".to_string(),
            epoch: Some("42".to_string()),
            vendor: Some("Vendor".to_string()),
            url: Some("https://some.invalid.url".to_string()),
            copyright: Some("2021 test".to_string()),
            build_arch: Some("noarch".to_string()),
            exclude_arch: Some("x86_64".to_string()),
            packager: Some("vv9k".to_string()),
            group: Some("group".to_string()),
            icon: Some("rpm.xpm".to_string()),
            summary: Some("short summary".to_string()),
            license: Some("MIT".to_string()),
            build_root: Some("/root/bld".to_string()),
            sources: vec!["source.tar.gz".to_string(), "source-2.tar.xz".to_string()],
            patches: vec!["patch.1".to_string(), "patch.2".to_string()],
            description: "very long summary...".to_string(),
            prep_script: Some("cat /etc/os-release".to_string()),
            check_script: Some("uptime".to_string()),
            build_script: Some(BUILD.to_string()),
            install_script: Some(INSTALL.to_string()),
            pre_script: Some("echo".to_string()),
            post_script: Some("false".to_string()),
            preun_script: Some("echo 123".to_string()),
            postun_script: Some("true".to_string()),
            files: vec!["/bin/test.bin".to_string(), "/docs/README".to_string()],
            doc_files: vec!["README".to_string()],
            license_files: vec!["LICENSE".to_string()],
            dir_files: vec![],
            conflicts: vec!["rpmspec2".to_string()],
            obsoletes: vec!["rpmspec-old".to_string()],
            provides: vec!["rpmspec".to_string()],
            requires: vec!["rust".to_string()],
            build_requires: vec!["rust".to_string(), "cargo".to_string()],
            config_noreplace: Some("%{_sysconfdir}/%{name}/%{name}.conf".to_string()),
            changelog: vec![],
            macros: vec![
                "githash 0ab32f".to_string(),
                "python(-c) import os".to_string(),
            ],
            auto_req_prov: Some(false),
        };

        assert_eq!(expect, spec);

        let expect_rendered = r#"Name:          rpmspec
Version:       0.1.0
Release:       1
Summary:       short summary
Epoch:         42
Vendor:        Vendor
URL:           https://some.invalid.url
Copyright:     2021 test
Packager:      vv9k
Group:         group
Icon:          rpm.xpm
License:       MIT
BuildRoot:     /root/bld
BuildArch:     noarch
ExcludeArch:   x86_64
Conflicts:     rpmspec2
obsoletes:     rpmspec-old
provides:      rpmspec
requires:      rust
BuildRequires: rust
BuildRequires: cargo
Patch0:        patch.1
Patch1:        patch.2
Source0:       source.tar.gz
Source1:       source-2.tar.xz
AutoReqProv:   No

%description
very long summary...

%prep
cat /etc/os-release

%build
echo 123 > test.bin
echo 321 > README

%install
install -m 755 test.bin /bin/test.bin
install -m 644 README /docs/README

%check
uptime

%pre
echo

%post
false

%preun
echo 123

%postun
true

%global githash 0ab32f
%global python(-c) import os

%files
"/bin/test.bin"
"/docs/README"

%doc
"README"

%license
"LICENSE"

%changelog
"#;
        let got = spec.render();
        assert_eq!(expect_rendered, got);
    }
}
