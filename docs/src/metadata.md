# Metadata (Required)
 - All the metadata and information needed for the build
 - **pkger** will install all dependencies listed in `build_depends`, depending on the OS type and choosing the appropriate package manager for each supported distribution. Default dependencies like `gzip` or `git` might be installed depending on the target job type. To skip installation of default dependencies add `skip_default_deps = true` to `[metadata]`
 - Below example recipe will be built for 2 images `centos8` and `debian10`. Each image also specifies the target that should be built using it.
 - If `git` is provided as a field, the repository that it points to will be automatically extracted to `$PKGER_OUT_DIR`, otherwise `pkger` will try to fetch `source`.
 - If `source` starts with a prefix like `http` or `https` the file that if points to will be downloaded. If the file is an archive like `.tar.gz` or `.tar.xz` it will be directly extracted to `$PKGER_BLD_DIR`, otherwise the file will be copied to the directory untouched.

## required fields

```toml
[metadata]
name = "pkger"
version = "0.1.0"
description = "A package building tool utilizing Docker"
license = "MIT"
```

## optional fields

```toml
# optional if building with `--simple` targets
images = [
    { name = "centos8" , target = "rpm" },
    { name = "debian10", target = "deb" }
]
```


#### common
```toml
release = "1" # defaults to "0"

epoch = "42"

source = "" # remote source or file system location

git = "https://github.com/wojciechkepka/pkger.git" # will default to branch = "master"
# or specify a branch like this:
# git = { url = "https://github.com/wojciechkepka/pkger.git", branch = "dev" }

maintainer = "Wojciech Kępka <wojciech@wkepka.dev>"

# The website of the package being built
url = "https://github.com/wojciechkepka/pkger"

arch = "x86_64" # defaults to `noarch` on RPM and `all` on DEB, `x86_64` automatically converted to `amd64` on DEB...

skip_default_deps = true # skip installing default dependencies, it might break the builds

exclude = ["share", "info"] # directories to exclude from final package

group = "" # acts as Group in RPM or Section in DEB build
```

#### dependencies
This fields can be specified as arrays:
```toml
depends   = []
conflicts = []
provides  = []
```
Or specified per image as a map:
```toml
[metadata.build_depends]
# common dependencies shared across all images
all      = ["gcc", "pkg-config", "git"]

# dependencies for custom images
centos8  = ["cargo", "openssl-devel"]
debian10 = ["curl", "libssl-dev"]
```

if running a simple build and there is a need to specify dependencies for the target add dependencies
for one of this images:
```toml
pkger-rpm = ["cargo"]
pkger-deb = ["curl"]
pkger-pkg = ["cargo"]
pkger-gzip = []

```

If specifying some deps as array and some as maps the arrays always have to come before the maps, otherwise TOML breaks


#### DEB fields
```toml
[metadata.deb]
priority = ""
installed_size = ""
built_using = ""
essential = true

# same as all other dependencies but deb specific
pre_depends = []
recommends = []
suggests = []
breaks = []
replaces = []
enchances = []
```

#### RPM fields
```toml
[metadata.rpm]
vendor = ""
icon = ""
summary = "shorter description" # if not provided defaults to value of `description`
config_noreplace = "%{_sysconfdir}/%{name}/%{name}.conf"

pre_script = ""
post_script = ""
preun_script = ""
postun_script = ""

# acts the same as other dependencies - can be passed as array
# obsoletes = ["foo"]
# or as a map per image at the end of rpm fields definition
[metadata.rpm.obsoletes]
centos8 = ["foo"]
```

#### PKG fields
```toml
[metadata.pkg]
# location of the script in $PKGER_OUT_DIR that contains pre/post install/upgrade/remove functions
# to be included in the final pkg
install = ".install"

# A list of files to be backed up when package will be removed or upgraded
backup = ["/etc/pkger.conf"]

# A list of packages that this package replaces
replaces = []

# This are dependencies that this package needs to offer full functionality.
# Each dependency should contain a short description in this format:
optdepends = [
    "libpng: PNG images support",
    "alsa-lib: sound support"
]
```
