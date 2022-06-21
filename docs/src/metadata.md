# Metadata (Required)

Contains all fields that describe the package being built.

## required fields

```yaml
metadata:
  name: pkger
  description: pkger
  license: MIT
  version: 0.1.0
```

## optional fields

To specify which images a recipe should use add images parameter with a list of image targets. This field is ignored
when building with `--simple` flag.

```yaml
  images: [ rocky, debian ]
```

You can also specify that all images apply to this recipe with:

```yaml
  all_images: true
```

### sources

This fields are responsible for fetching the files used for the build. When both `git` and `source` are specified
**pkger** will fetch both to the build directory.

If `source` starts with a prefix like `http` or `https` the file that if points to will be downloaded. If the file is an
archive like `.tar.gz` or `.tar.xz` or `.zip` it will be directly extracted to
[`$PKGER_BLD_DIR`](./env.md#pkger-variables), otherwise the file will be copied to the directory untouched.

```yaml
  source: "" # remote source or file system location

  # can also specify multiple sources:

  source:
    - 'http://some.website.com/file.tar.gz'
    - some_dir   # relative path will be prefixed with recipe directory
    - /some/absolute/path # can be a directory or a file

  git: https://github.com/vv9k/pkger.git # will default to branch = "master"

  # or specify a branch like this:
  git:
    url: https://github.com/vv9k/pkger.git
    branch: dev
```

[Environment variables](./env.md) are available for this fields so this is possible:
```yaml
  source: "https://github.com/vv9k/${RECIPE}/${RECIPE_VERSION}"
```


### common

Optional fields shared across all targets.

```yaml
  release: "1" # defaults to "0"

  epoch: "42"

  maintainer: "vv9k"

# The website of the package being built
  url: https://github.com/vv9k/pkger

  arch: x86_64 # defaults to `noarch` on RPM and `all` on DEB, `x86_64` automatically converted to `amd64` on DEB...

  skip_default_deps: true # skip installing default dependencies, it might break the builds

  exclude: ["share", "info"] # directories to exclude from final package

  group: "" # acts as Group in RPM or Section in DEB build
```


### dependencies

Common fields that specify dependencies, conflicts and provides will be added to the spec of the final package. 

This fields can be specified as arrays:
```yaml
  depends: []
  conflicts: []
  provides: []
```
Or specified per image as a map below.

**pkger** will install all dependencies listed in `build_depends`, choosing an appropriate package manager for each
supported distribution. Default dependencies like `gzip` or `git` might be installed depending on the target job type.

```yaml
  build_depends:
    # common dependencies shared across all images
    all: ["gcc", "pkg-config", "git"]

    # dependencies for custom images
    rocky: ["cargo", "openssl-devel"]
    debian: ["curl", "libssl-dev"]
```

To specify same dependencies for multiple images join the images by `+` sign like this:
```yaml
    rocky+fedora34: [ cargo, openssl-devel ]
    ubuntu20+debian: [ libssl-dev ]
    # you can later specify dependencies just for this images
    debian: [ curl ]
```

if running a simple build and there is a need to specify dependencies for the target add dependencies for one of this
images:

```yaml
    pkger-rpm+pkger-apk+pkger-pkg: ["cargo"]
    pkger-deb: ["curl"]
    pkger-gzip: []
```

A custom image, for example `rocky`, will also use dependecies defined for `pkger-rpm`. The same will apply for all rpm based images (or images that have their target specified to RPM in the [configuration](./configuration.md))


### Patches

To apply patches to the fetched source code specify them just like dependencies. Patches can be specified as just file
name in which case **pkger** will look for the patch in the recipe directory, if the path is absolute it will be read
directly from the file system and finally if the patch starts with an `http` or `https` prefix the patch will be fetched
from remote source.

```yaml
  patches:
    - some-local.patch
    - /some/absolute/path/to.patch
    - https://someremotesource.com/other.patch
    - patch: with-strip-level.patch
      images: [ debian ] # specify the images that this patch should be aplied on
      strip: 2 # this specifies the number of directories to strip before applying the patch (known as -pN or --stripN option in UNIX patch tool
```
