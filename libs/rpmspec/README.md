# rpmspec-rs

[![GitHub Actions](https://github.com/wojciechkepka/rpmspec-rs/workflows/Main/badge.svg)](https://github.com/wojciechkepka/rpmspec-rs/actions) [![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE) [![Released API docs](https://docs.rs/rpmspec/badge.svg)](http://docs.rs/rpmspec)

> Crate for RPM spec generation in Rust

## Usage

This crate provides a simple builder interface for RPM spec files generation.

Here's an example of building a spec file from scratch (some fields are ommited):

```rust
use rpmspec::RpmSpec;

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error + 'static + Sync + Send>> {
    let spec = RpmSpec::builder()
        .name("rpmspec")
        .license("MIT")
        .summary("short summary")
        .description("very long summary...")
        .build_script(BUILD)
        .install_script(INSTALL)
        .prep_script(r#"cat /etc/os-release"#)
        .check_script("uptime")
        .url("https://github.com/wojciechkepka/rpmspec")
        .version("0.1.0")
        .release("1")
        .epoch("42")
        .vendor("Wojciech Kępka")
        .packager("Wojciech Kępka <wojciech@wkepka.dev>")
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
        .build();

    // you can later render it to a string like so:
    let _rendered = spec.render()?;

    // or save it directly to a file
    spec.save_to("/tmp/RPMSPEC")?;

    Ok(())
}

```

## License
[MIT](https://github.com/wojciechkepka/rpmspec-rs/blob/master/LICENSE)
