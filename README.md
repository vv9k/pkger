# pkger 📦🐳
[![Travis CI](https://travis-ci.org/wojciechkepka/pkger.svg?branch=master)](https://travis-ci.org/wojciechkepka/pkger/builds)  
Package building tool utilizing Docker.

The main purpose of pkger is to automate building `.rpm` or `.deb` (perhaps more in the future) packages on multiple operating systems, versions and architectures.

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
   - When `pkger` finishes building the package it will create a directory `$output_dir/$os/$ver/` where it will put built `.rpm` or `.deb` package. 
     - `$os` and `$ver` are taken from container during build

## Recipe
The recipe is divided into 3 parts:
 - ### Info
   - All the metadata and information needed for the build
   - `pkger` will install all dependencies listed in `depends`(for Debian based) or `depends_rh`(for RedHat based) depending on the Os type choosing the appropriate package manager for each supported distribution.
   - This recipe will be built for 2 images `centos8` and `debian10`.
   - `pkger` will look for the image directory in f.e. `$images_dir/centos8`.
```
[info]
name = "pkger"
description = "pkger"
arch = "x86_64"
license = "MIT"
version = "0.0.5"
revision = "0"
source = ""
git = "https://github.com/wojciechkepka/pkger.git"
depends = ["curl", "gcc", "pkg-config", "libssl-dev"]
depends_rh = ["curl", "gcc", "pkg-config", "openssl-devel"]
exclude = ["share", "info"]
provides = ["pkger"]
images = [
	"centos8",
	"debian10",
]
```
 - ### Build
   - All build steps presented as a list of string
```
[build]
steps = [
	"curl -o /tmp/install_rust.sh https://sh.rustup.rs",
	"sh /tmp/install_rust.sh -y --default-toolchain stable",
	"mkdir -p /opt/pkger/bin",
	"/root/.cargo/bin/cargo build --target-dir /tmp",
]
```
 - ### Install
   - All install steps presented as a list of string
```
[install]
steps = [
	"mv /tmp/debug/pkger /opt/pkger/bin"
]
```
 - ### Finish
   - `files` specifies the directory where all installed files are
     - in the example below if there is a file `/opt/pkger/usr/bin/file` it will be inserted into the package under `install_dir` path. In this case `/opt/pkger/usr/bin/file` will be installed to `/usr/bin/file`
```
[finish]
files = "/opt/pkger"
install_dir = "/"
```
 - ### Env (Optional)
   - Set environment variables to use in recipes during build
```
[env]
HTTPS_PROXY = "http://proxy.domain.com:1234"
RUST_LOG = "trace"
```

## Usage
To install `pkger` clone and build this repository with `crago build --release`.

To use `pkger` you need a [docker daemon running on a tcp port](https://success.docker.com/article/how-do-i-enable-the-remote-api-for-dockerd).
After that run:
 - `pkger -d $docker_address -c $config_file [RECIPES]`
 - Substitute `$docker_address` with address like `http://0.0.0.0:2376`
 - Substitute `$config_file` with path to the config file 
 - Add any amount of recipes whitespace separated at the end

To get some informative output run with `RUST_LOG=pkger=trace` env variable set

## Example
Example configuration, recipe and file structure can be found in [`example` directory of `master` branch](https://github.com/wojciechkepka/pkger/tree/master/example)

## License
[MIT](https://github.com/wojciechkepka/pkger/blob/master/LICENSE)
