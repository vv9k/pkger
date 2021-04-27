# Scripts

**pkger** has 3 defined build phases - *configure*, *build* and *install* of which only *build* is required to create a package.  

Each phase has field called `steps` that takes an array of steps to execute during a given phase. A step can be a simple string that will be executed in the default shell like `"echo 123"` or an entry that specifies on what targets it should be executed like:
```toml
{ cmd = "echo only on deb targets", deb = true }
```

To set a working directory during the script phase set the `working_dir` parameter like so:
```toml
working_dir = "/tmp"
```

To use a different shell to execute each command set the `shell` parameter:
```toml
shell = "/bin/bash" # optionally change default `/bin/sh`
```

## configure (Optional)

Optional configuration steps. If provided the steps will be executed before the build phase.

```toml
[configure]
shell = "/bin/bash"
steps = [
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
]
```

## build (Required)

This is the phase where the package should be assembled/compiled/linked and so on. All steps executed during the build will have the working directory seto to [`$PKGER_BLD_DIR`](./env.md#pkger-variables). This directory will contain either extracted sources if `source` is specified in [metadata](./metadata.md#optional-fields) or a git repository if `git` was specified.

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

Optional steps that (if provided) will be executed after the build phase. Working directory of each step will be set to [`$PKGER_OUT_DIR`](./env.md#pkger-variables) so you can use relative paths with commands like install. Each file that ends up in [`$PKGER_OUT_DIR`](./env.md#pkger-variables) will be available in the final package unless explicitly excluded by `exclude` field in [metadata](./metadata.md#optional-fields). So in the example below, the file that is installed will be available as `/usr/bin/pkger` with permissions preserved.

```toml
[install]
steps = [
    "install -m755 $PKGER_BLD_DIR/target/release/pkger usr/bin/pkger"
]
```
