# Metadata (Required)

Contains all fields that describe the package being built.

## required fields

```toml
[metadata]
name = "pkger"
version = "0.1.0"
description = "A package building tool utilizing Docker"
license = "MIT"
```

## optional fields

To specify which images a recipe should use add images parameter with a list of image targets. This field is ignored when building with `--simple` flag.

```toml
images = [
    { name = "centos8" , target = "rpm" },
    { name = "debian10", target = "deb" }
]
```

### sources

This fields are responsible for fetching the files used for the build. When both `git` and `source` are specified **pkger** will fetch both to the build directory.

If `source` starts with a prefix like `http` or `https` the file that if points to will be downloaded. If the file is an archive like `.tar.gz` or `.tar.xz` or `.zip` it will be directly extracted to [`$PKGER_BLD_DIR`](./env.md#pkger-variables), otherwise the file will be copied to the directory untouched.

```toml
source = "" # remote source or file system location

git = "https://github.com/wojciechkepka/pkger.git" # will default to branch = "master"
# or specify a branch like this:
# git = { url = "https://github.com/wojciechkepka/pkger.git", branch = "dev" }
```


### common

Optional fields shared across all targets.

```toml
release = "1" # defaults to "0"

epoch = "42"

maintainer = "Wojciech Kępka <wojciech@wkepka.dev>"

# The website of the package being built
url = "https://github.com/wojciechkepka/pkger"

arch = "x86_64" # defaults to `noarch` on RPM and `all` on DEB, `x86_64` automatically converted to `amd64` on DEB...

skip_default_deps = true # skip installing default dependencies, it might break the builds

exclude = ["share", "info"] # directories to exclude from final package

group = "" # acts as Group in RPM or Section in DEB build
```


### dependencies

Common fields that specify dependencies, conflicts and provides will be added to the spec of the final package. 

This fields can be specified as arrays:
```toml
depends   = []
conflicts = []
provides  = []
```
Or specified per image as a map below.

**pkger** will install all dependencies listed in `build_depends`, choosing an appropriate package manager for each supported distribution. Default dependencies like `gzip` or `git` might be installed depending on the target job type.

```toml
[metadata.build_depends]
# common dependencies shared across all images
all      = ["gcc", "pkg-config", "git"]

# dependencies for custom images
centos8  = ["cargo", "openssl-devel"]
debian10 = ["curl", "libssl-dev"]
```

if running a simple build and there is a need to specify dependencies for the target add dependencies for one of this images:

```toml
pkger-rpm = ["cargo"]
pkger-deb = ["curl"]
pkger-pkg = ["cargo"]
pkger-gzip = []

```

