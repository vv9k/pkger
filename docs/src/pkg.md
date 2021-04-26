# PKG fields

Optional fields that will be used when building a PKG package.

```toml
[metadata.pkg]
# location of the script in `$PKGER_OUT_DIR` that contains pre/post install/upgrade/remove functions
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
