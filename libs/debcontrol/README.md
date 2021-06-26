# deb-control-rs

[![GitHub Actions](https://github.com/wojciechkepka/deb-control-rs/workflows/Main/badge.svg)](https://github.com/wojciechkepka/deb-control-rs/actions) [![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE) [![Released API docs](https://docs.rs/deb-control/badge.svg)](http://docs.rs/deb-control)

> Crate for DEB/control file generation in Rust

## Usage

This crate provides a simple builder interface for DEB/control files generation. There are two types of builders: binary and source. To access them use the associated functions on `DebControlBuilder`, for example:
```rust
use deb_control::DebControlBuilder;

DebControlBuilder::binary_package_builder();
// or
DebControlBuilder::source_package_builder();
```

Here's an example of building a binary DEB/control from scratch:
```rust
use deb_control::DebControlBuilder;

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error + 'static + Sync + Send>> {
    let control = DebControlBuilder::binary_package_builder("debcontrol")
        .source("package.tar.gz")
        .version("1")
        .architecture("any")
        .maintainer("Wojciech Kępka <wojciech@wkepka.dev>")
        .description("crate for DEB/control file generation")
        .essential(true)
        .section("devel")
        .homepage("https://github.com/wojciechkepka/debcontrol")
        .built_using("rustc")
        .add_pre_depends_entries(vec!["rustc", "cargo"])
        .add_depends_entries(vec!["rustc", "cargo"])
        .add_conflicts_entries(vec!["rustc", "cargo"])
        .add_provides_entries(vec!["rustc", "cargo"])
        .add_replaces_entries(vec!["rustc", "cargo"])
        .add_enchances_entries(vec!["rustc", "cargo"])
        .add_provides_entries(vec!["debcontrol"])
        .build();

    // you can later render it to a string like so:
    let _rendered = control.render()?;

    // or save it directly to a file
    control.save_to("/tmp/CONTROL")?;

    Ok(())
}

```


## License
[MIT](https://github.com/wojciechkepka/deb-control-rs/blob/master/LICENSE)
