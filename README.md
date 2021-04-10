# pkger 📦
[![Build Status](https://github.com/wojciechkepka/pkger/workflows/pkger%20CI/badge.svg)](https://github.com/wojciechkepka/pkger/actions?query=workflow%3A%22pkger+CI%22)

pkger is a tool to automate building RPMs and DEBs as well as other artifacts on multiple Linux distributions, versions and architectures.

## How it works

pkger has 2 concepts - images and recipes. Each recipe is a sort of build mainfest that allows *pkger* to create the final artifact. Images are directories that contain a `Dockerfile` as well as optional other files. 

## Recipe

The recipe is divided into 2 parts:
 - ### metadata
   - All the metadata and information needed for the build
   - `pkger` will install all dependencies listed in `build_depends`, depending on the OS type and choosing the appropriate package manager for each supported distribution.
   - Below example recipe will be built for 2 images `centos8` and `debian10`. Each image also specifies the target that should be built using it. Currently available targets are: *rpm*, *deb*, *gzip*.
   - Special syntax for unique dependencies across OSes is used to correctly install `openssl-devel` on *CentOS 8* and `libssl-dev` on *Debian 10*
```
[metadata]
name = "pkger"
description = "pkger"
arch = "x86_64"
license = "MIT"
version = "0.0.5"
revision = "0"
source = ""
git = "https://github.com/wojciechkepka/pkger.git"
build_depends = ["curl", "gcc", "pkg-config", "debian10:{libssl-dev},centos8:{openssl-devel}"]
depends = []
exclude = ["share", "info"]
provides = ["pkger"]
images = [
	{ name = "centos8", target = "rpm" },
	{ name = "debian10", target = "deb"}
]
```
 - ### build
   - All build steps presented as a list of string
   - To execute a command only in a container with specific image/images you can write:
     - `pkger%:centos8 echo 'test'` for a single image
     - `pkger%:{centos8,debian10} echo 'test'` or `pkger%:{centos8, debian10} echo 'test'` for multiple images
   - After successfully running all steps pkger will assemble the final artifact from `$PKGER_BLD_DIR` directory
```
[build]
steps = [
	"curl -o /tmp/install_rust.sh https://sh.rustup.rs",
	"sh /tmp/install_rust.sh -y --default-toolchain stable",
	"mkdir -p /opt/pkger/bin",
	"/root/.cargo/bin/cargo build --target-dir /tmp",
]
```
 - ### Env (Optional)
   - Set environment variables to use in recipes during build
   - `pkger` also provides some env variables to use for adding logic to the build part
     - `$PKGER_OS` the os of current container
     - `$PKGER_OS_VERSION` version of current os
     - `$PKGER_BLD_DIR` the build directory with fetched source in a container
```
[env]
HTTPS_PROXY = "http://proxy.domain.com:1234"
RUST_LOG = "trace"
```

## Config

Config file has a following structure:
```
images_dir = ""
recipes_dir = ""
output_dir = ""
```
 - `images_dir` - directory with images
   - Each image is a directory containing a `Dockerfile` and files to be imported with it
   - Image name is the directory name
 - `recipes_dir` - directory with recipes
   - Each recipe is a directory containing a `recipe.toml` file and source files (if not remote) 
 - `output_dir` - directory with built packages
   - When `pkger` finishes building the package it will create a directory `$output_dir/$PKGER_OS/$PKGER_OS_VERSION/` where it will put the built artifact

## Usage

To install `pkger` clone and build this repository with `cargo build --release`.

To use `pkger` you need a [docker daemon listening on a tcp or unix port](https://success.docker.com/article/how-do-i-enable-the-remote-api-for-dockerd).
After that run:
 - `pkger -d $docker_address -c $config_file [RECIPES]`
 - Substitute `$docker_address` with address like `http://0.0.0.0:2376`
 - Substitute `$config_file` with path to the config file 
 - Add any amount of recipes whitespace separated at the end

To debug run with `RUST_LOG=pkger=trace` env variable set. By default `pkger` will set `RUST_LOG=pkger=info` to display basic output.

## Example

 - Example configuration, recipe can be found in [`example` directory of `master` branch](https://github.com/wojciechkepka/pkger/tree/master/example)
 - Example file structure:
```
example_structure/
├── conf.toml
├── images
│   ├── centos8
│   │   └── Dockerfile
│   └── debian10
│       ├── Dockerfile
│       └── some_archive.tar.gz
├── out
│   ├── centos
│   │   └── 8
│   │       ├── curl_7.67.0-0.rpm
│   │       └── nginx_1.17.6-0.rpm
│   └── debian
│       └── 10
│           ├── curl_7.67.0-0.deb
│           └── nginx_1.17.6-0.deb
├── pkger
└── recipes
    ├── curl
    │   └── recipe.toml
    └── nginx
        └── recipe.toml
```

## License
[MIT](https://github.com/wojciechkepka/pkger/blob/master/LICENSE)
