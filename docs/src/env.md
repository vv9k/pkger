# env (Optional)
 - Set environment variables to use in recipes during build
 - **pkger** also provides some environment variables to use during the recipe build
   - `$PKGER_OS` the os of current container
   - `$PKGER_OS_VERSION` version of current os
   - `$PKGER_BLD_DIR` the build directory with fetched source in the container
   - `$PKGER_OUT_DIR` the final directory from which **pkger** will copy files to target package

```toml
[env]
HTTPS_PROXY = "http://proxy.domain.com:1234"
RUST_LOG = "trace"
```
