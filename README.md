# pkger 📦
[![Build Status](https://github.com/wojciechkepka/pkger/workflows/pkger%20CI/badge.svg)](https://github.com/wojciechkepka/pkger/actions?query=workflow%3A%22pkger+CI%22)

**pkger** is a tool that automates building *RPMs*, *DEBs* and other packages on multiple *Linux* distributions, versions and architectures with the help of Docker.

![Example output](https://github.com/wojciechkepka/pkger/blob/master/assets/example_output.png)

## How it works

**pkger** has 2 concepts - images and recipes. Each recipe is a sort of build mainfest that allows **pkger** to create the final package. Images are directories that contain a `Dockerfile` as well as optional other files that might get included in the image build phase. 

## Recipe

The recipe is divided into 2 required (*metadata*, *build*) and 3 optional (*config*, *install*, *env*) parts:
 - ### metadata
   - All the metadata and information needed for the build
   - **pkger** will install all dependencies listed in `build_depends`, depending on the OS type and choosing the appropriate package manager for each supported distribution. Default dependencies like `gzip` or `git` might be installed depending on the target job type. To skip installation of default dependencies add `skip_default_deps = true` to `[metadata]`
   - Below example recipe will be built for 2 images `centos8` and `debian10`. Each image also specifies the target that should be built using it.
   - Special syntax for unique dependencies across OSes is used to correctly install `openssl-devel` on *CentOS 8* and `libssl-dev` on *Debian 10*
   - If `git` is provided as a field, the repository that it points to will be automatically extracted to `$PKGER_OUT_DIR`, otherwise `pkger` will try to fetch `source`.
   - If `source` starts with a prefix like `http` or `https` the file that if points to will be downloaded. If the file is an archive like `.tar.gz` or `.tar.xz` it will be directly extracted to `$PKGER_BLD_DIR`, otherwise the file will be copied to the directory untouched.
```toml
[metadata]
#### required common fields
name = "pkger"
version = "0.1.0"
description = "A package building tool utilizing Docker"
license = "MIT"
images = [
	{ name = "centos8" , target = "rpm" },
	{ name = "debian10", target = "deb" }
]


#### optional common
source = "" # remote source or file system location

git = "https://github.com/wojciechkepka/pkger.git" # will default to branch = "master"
# or specify a branch like this:
# git = { url = "https://github.com/wojciechkepka/pkger.git", branch = "dev" }

maintainer = "Wojciech Kępka <wojciech@wkepka.dev>" # defaults to `none` for DEB build as it's required
arch = "x86_64" # defaults to `noarch` on RPM and `all` on DEB, `x86_64` automatically converted to `amd64` on DEB...
skip_default_deps = true # skip installing default dependencies, it might break the builds
exclude = ["share", "info"] # directories to exclude from final package

build_depends = [ # dependencies to install before build phase
 "curl",
 "gcc",
 "pkg-config",
 "debian10:{libssl-dev},centos8:{openssl-devel}" # custom syntax to install packages with different names
                                                 # on different images
]

depends = []
conflicts = []
provides = []

#### optional DEB fields
section = ""
priority = ""

#### RPM fields
release = "1" # defaults to 0
obsoletes = []
summary = "shorter description" # if not provided defaults to value of `description`
```
 - ### configure (Optional)
 - Optional configuration steps. If provided the steps will be executed before the build phase.
```toml
[configure] # optional
steps = [
	"curl -o /tmp/install_rust.sh https://sh.rustup.rs",
	"sh /tmp/install_rust.sh -y --default-toolchain stable",
]
```
 - ### build
   - All build steps presented as a list of strings
   - Steps will be executed with a working directory set to `$PKGER_BLD_DIR`
   - To execute a command only in a container with specific image/images you can write:
     - `pkger%:centos8 echo 'test'` for a single image
     - `pkger%:{centos8,debian10} echo 'test'` or `pkger%:{centos8, debian10} echo 'test'` for multiple images
   - After successfully running all steps **pkger** will assemble the final package from `$PKGER_BLD_DIR` directory
```toml
[build] # required
steps = [
	"$HOME/.cargo/bin/cargo build .",
]
```
 - ### install (Optional)
   - Optional installation steps. If provided the steps will be executed after the build phase.
   - Working directory will be set to `$PKGER_OUT_DIR` by default so you can use relative paths during install

```toml
[install] # optional
steps = [
    "install -m755 pkger usr/bin/pkger"
]
```
 - ### Env (Optional)
   - Set environment variables to use in recipes during build
   - **pkger** also provides some environment variables to use during the recipe build
     - `$PKGER_OS` the os of current container
     - `$PKGER_OS_VERSION` version of current os
     - `$PKGER_BLD_DIR` the build directory with fetched source in the container
     - `$PKGER_OUT_DIR` the final directory from which **pkger** will copy files to target package
```toml
[env] # optional
HTTPS_PROXY = "http://proxy.domain.com:1234"
RUST_LOG = "trace"
```

## Final package

Currently available targets are: *RPM*, *DEB*, *GZIP*.  

After executing build script (or install if provided), **pkger** will copy all files from `$PKGER_OUT_DIR` to final package. So for example if this directory contains a file `$PKGER_OUT_DIR/usr/bin/pkger` this file will be added to the package as `/usr/bin/pkger`.

## Config

Config file has a following structure:
```toml
images_dir = ""
recipes_dir = ""
output_dir = ""
docker = "unix:///var/run/docker.sock" # optional
```
 - `images_dir` - directory with images
   - Each image is a directory containing a `Dockerfile` and files to be imported with it
   - Image name is the directory name
 - `recipes_dir` - directory with recipes
   - Each recipe is a directory containing a `recipe.toml` file and source files (if not remote) 
 - `output_dir` - directory with built packages
   - When **pkger** finishes building the package it will create a directory `$output_dir/$PKGER_OS/$PKGER_OS_VERSION/` where it will put the built package
 - `docker` - specify docker uri in configuration.

If an option is available as both configuration parameter and cli argument **pkger** will favour the arguments passed during startup.

## Usage

To install **pkger** clone and build this repository with `cargo build --release`.

To use **pkger** you need a [docker daemon listening on a tcp or unix port](https://success.docker.com/article/how-do-i-enable-the-remote-api-for-dockerd).

To build a package use
 - `pkger build [RECIPES]`
 - If `-c` is not provided **pkger** will look for the configuration file in the default location - `$HOME/.pkger.toml`. If the user has no home directory then as the last resort it will try to use `.pkger.toml` in current working directory as config path.
 - Add any amount of recipes whitespace separated at the end. If no recipe name is provided, all recipes will be queued for a build.

By default **pkger** will display basic output as hierhical log with level set to `INFO`. To debug run with `-d` or `--debug` option. To surpress all output except for errors add `-q` or `--quiet`. To manually set log level set `RUST_LOG` env variable to a value like `pkger=debug` with debug replaced with the desired log level.

To decide what parts of events are displayed use the `--hide` flag that takes a filter string as input and tells **pkger** what fields to display. Each character of filter string is responsible for a single part of output. Characters are case insensitive, the order doesn't matter and duplicates are silently ignored. Available modules are:
 - `d` hides the timestamp
 - `f` hides the fields in spans (the values between curly braces like `{id = vw89wje92}`)
 - `l` hides the level
 - `s` hides the spans entirely

To generate a recipe declaratively from CLI use subcommand `gen-recipe`. By default it requires only the name of the package and prints the recipe to stdout. If `output_dir` is provided **pkger** will create a directory with the name of the package and a `recipe.toml` containing generated recipe.

Example generated recipe printed to stdout:
```
> pkger gen-recipe test --arch x86_64 --description "A very interesting package..." \
                        --provides test-bin --version 1.0.0 --build-depends curl make \
                        -- license MIT
[metadata]
name = "test"
version = "1.0.0"
description = "A very interesting package..."
license = "MIT"
images = []
arch = "x86_64"
build_depends = ["curl", "make"]
provides = ["test-bin"]

[build]
steps = []

```

## Example

 - Example configuration and recipe can be found in [`example` directory of `master` branch](https://github.com/wojciechkepka/pkger/tree/master/example)
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
