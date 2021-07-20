# Build a package

Currently available targets are: **RPM**, **DEB**, **PKG**, **GZIP**.  

### Simple build

To build a simple package using **pkger** use:
 - `pkger build --simple [TARGETS] -- [RECIPES]`

### Custom images build

To use [custom images](./images.md) drop the `--simple` parameter and just use:
 - `pkger build [RECIPES]`

For this to have any effect the recipes have to have image targets defined (more on that [here](./metadata.md#optional-fields))

### Output

After successfully building a package **pkger** will put the output artifact to `output_dir` specified in
[configuration](./configuration.md) joined by the image name that was used to build the package.
Each image will have a separate directory with all of its output packages.
