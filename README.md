# pkger 📦🐳
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
   - `pkger` will install all dependencies listed in `depends` choosing the appropriate package manager for each supported distribution.
   - This recipe will be built for all 3 images `centos`, `fedora`, `ubuntu_latest`.
   - `pkger` will look for the image directory in f.e. `$images_dir/centos`.
```
[info]
name = "curl"
description = "curl"
arch = "x86_64"
license = "null"
version = "7.67.0"
revision = "0"
source = "curl-7.67.0.tar.gz"
depends = ["gcc", "make", "patch", "binutils", "strace"]
exclude = ["share", "info"]
provides = ["curl"]
images = [
	"centos",
	"fedora",
	"ubuntu_latest",
]
```
 - ### Build
   - All build steps presented as a list of string
```
[build]
steps = [
	"./curl-7.67.0/configure --prefix=/opt/curl/7.67.0",
	"make"
]
```
 - ### Install
   - All install steps presented as a list of string
   - `destdir` which is the directory where the installed files are. All the steps from `build` and `install` must result in built files in `destdir` which will then be archived and built into a package.
```
[install]
steps = ["make install"]
destdir = "/opt/curl/7.67.0"
```

## Usage
To install `pkger` run `cargo install pkger` or clone and build this repository with `crago build --release`.

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
