# env (Optional)

Optional environment variables that should be available during the [scripts](./scripts.md) phase and
in some metadata fields.

```yaml
env:
  HTTPS_PROXY: http://proxy.domain.com:1234
  RUST_LOG: trace
```

# **pkger** variables
Some variables will be available to use during the build like:
 - `$PKGER_OS` the distribution of current container
 - `$PKGER_OS_VERSION` version of the distribution if applies
 - `$PKGER_BLD_DIR` the build directory with fetched source or git repo in the container
 - `$PKGER_OUT_DIR` the final directory from which **pkger** will copy files to target package
 - `$RECIPE` the name of the recipe that is built
 - `$RECIPE_VERSION` the version of the recipe
 - `$RECIPE_RELEASE` the release of the recipe
