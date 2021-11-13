# APK fields

Optional fields that will be used when building a APK package.

```yaml
  apk:
    install: "$pkgname.pre-install $pkgname.post-install"
    
    # A list of packages that this package replaces
    replaces: []

    # A list of dependencies for the check phase
    checkdepends: []

    # If not provided a new generated key will be used to
    # sign the package
    private_key: "/location/of/apk_signing_key"
```
