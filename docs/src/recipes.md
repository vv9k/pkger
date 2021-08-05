# Recipes

Each recipe is a directory containing at least a `recipe.yml` or `recipe.yaml` file located at `recipes_dir` specified
in the [configuration](./configuration.md).

The recipe is divided into 2 required (*metadata*, *build*) and 3 optional (*config*, *install*, *env*) parts.
To read more on each topic select a subsection in the menu.

Here's an example working recipe for **pkger**:

```yaml
metadata:
  name: pkger
  description: pkger
  arch: x86_64
  license: MIT
  version: 0.1.0
  maintainer: "Wojciech Kępka <wojciech@wkepka.dev>"
  url: "https://github.com/vv9k/pkger"
  git: "https://github.com/vv9k/pkger.git"
  provides: [ pkger ]
  depends:
    pkger-deb: [ libssl-dev ]
    pkger-rpm: [ openssl-devel ]
  build_depends:
    all: [ gcc, pkg-config ]
    pkger-deb: [ curl libssl-dev ]
    pkger-rpm: [ cargo ]
    pkger-pkg: [ cargo ]
env:
  RUSTUP_URL: https://sh.rustup.rs
configure:
  steps:
    - cmd: curl -o /tmp/install_rust.sh $RUSTUP_URL
      deb: true
    - cmd: sh /tmp/install_rust.sh -y --default-toolchain stable
      deb: true
build:
  steps:
    - cmd: cargo build --color=never
      rpm: true
      pkg: true
    - cmd: $HOME/.cargo/bin/cargo build --color=never
      deb: true
install:
  steps:
    - cmd: dir -p usr/bin
    - cmd: install -m755 $PKGER_BLD_DIR/target/debug/pkger usr/bin/

```
