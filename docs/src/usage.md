# Build a package

Currently available targets are: **RPM**, **DEB**, **PKG**, **GZIP**.  

### Simple build

To build a simple package using **pkger** use:
```shell
pkger build --simple [TARGETS] -- [RECIPES]
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
pkger build -s rpm deb pkg gzip -- recipe1
```

#### Build only RPM package:
```shell
pkger build -s rpm -- recipe1
```

### Output

After successfully building a package **pkger** will put the output artifact to `output_dir` specified in
[configuration](./configuration.md) joined by the image name that was used to build the package.
Each image will have a separate directory with all of its output packages.
