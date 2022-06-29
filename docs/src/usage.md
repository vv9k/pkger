# Build a package

Currently available targets are: **rpm**, **deb**, **pkg**, **apk**, **gzip**.

### Simple build

To build a simple package using **pkger** use:
```shell
pkger build --simple [TARGETS] -- [RECIPES]
```

When using a simple build following linux distributions will be used for build images:
 - rpm: `rockylinux/rockylinux:latest`
 - deb: `debian:latest`
 - pkg: `archlinux`
 - apk: `alpine:latest`
 - gzip: `debian:latest`

To override the default images set `custom_simple_images` like this:
```yaml
custom_simple_images:
  deb: ubuntu:18
  rpm: fedora:latest
```

### Custom images build

To use [custom images](./images.md) drop the `--simple` parameter and just use:
```shell
pkger build [RECIPES]
```

For this to have any effect the recipes have to have image targets defined (more on that [here](./metadata.md#optional-fields))

### Examples

#### Build a recipe for all supported images:
```shell
pkger build recipe

# or shorthand 'b' for 'build'
pkger b recipe
```

#### Build all recipes for all supported images
```shell
pkger build --all
```

#### Build multiple recipes on specified custom images:
```shell
pkger build -i custom-image1 custom-image2 -- recipe1 recipe2
```

#### Build simple RPM, DEB, PKG... packages:
```shell
pkger build -s rpm -s deb -s pkg -s gzip -- recipe1
```

#### Build only RPM package:
```shell
pkger build -s rpm -- recipe1
```

### Output

After successfully building a package **pkger** will put the output artifact to `output_dir` specified in
[configuration](./configuration.md) joined by the image name that was used to build the package.
Each image will have a separate directory with all of its output packages.
