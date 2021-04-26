# Build a package

Currently available targets are: **RPM**, **DEB**, **PKG**, **GZIP**.  

### Simple build

To build a simple package using **pkger** use:
 - `pkger build --simple [TARGETS] -- [RECIPES]`

### Custom images build

To use [custom images](./images.md) drop the `--simple` parameter and just use:
 - `pkger build [RECIPES]`

For this to have any effect the recipes have to have image targets defined (more on that [here](./metadata.md#optional-fields))
