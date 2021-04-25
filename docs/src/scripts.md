# Scripts

**pkger** has 3 defined build phases - *configure*, *build* and *install* of which only *build* is required to create a package.  

To set a working directory during the script phase set the `working_dir` variable like so:
```toml
working_dir = "/tmp"
```

To use a different shell to execute each command set the `shell` variable:
```toml
shell = "/bin/bash" # optionally change default `/bin/sh`
```

## configure (Optional)
 - Optional configuration steps. If provided the steps will be executed before the build phase.

```toml
[configure]
steps = [
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
]
```

## build (Required)
 - All build steps presented as a list of strings
 - Steps will be executed with a working directory set to `$PKGER_BLD_DIR`
 - After successfully running all steps **pkger** will assemble the final package from `$PKGER_BLD_DIR` directory
```toml
[build]
steps = [
    "$HOME/.cargo/bin/cargo build --release .",
    { images = ["debian10"], cmd = "echo 'hello from Debian'" }, # will only be executed on image `debian10`
    { rpm = true, cmd = "echo 'will only run on images with target == `rpm`'" }
    # same applies to other targets
    # { pkg = false, deb = true, gzip = false, cmd = "echo test" }
]
```

## install (Optional)
 - Optional installation steps. If provided the steps will be executed after the build phase.
 - Working directory will be set to `$PKGER_OUT_DIR` by default so you can use relative paths during install
```toml
[install]
steps = [
    "install -m755 $PKGER_BLD_DIR/target/release/pkger usr/bin/pkger"
]
```


After executing build script (or install if provided), **pkger** will copy all files from `$PKGER_OUT_DIR` to final package. So for example if this directory contains a file `$PKGER_OUT_DIR/usr/bin/pkger` this file will be added to the package as `/usr/bin/pkger`.
