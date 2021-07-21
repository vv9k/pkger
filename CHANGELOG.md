# 0.5.0
- If at least one task fails to build **pkger** will now return with exit code 1 [#66](https://github.com/wojciechkepka/pkger/pull/66)
- `--quiet` flag now suppresses only container output, normal info output is still available [#67](https://github.com/wojciechkepka/pkger/pull/67)

# 0.4.0
- Add an option to sign RPMs with a GPG key [#55](https://github.com/wojciechkepka/pkger/pull/55)
- Add an option to sign DEBs with a GPG key [#56](https://github.com/wojciechkepka/pkger/pull/56)
- Fix image caching
- Add `forward_ssh_agent` configuration option to forward the SSH authentication socket from the host machine to 
  the container. [#58](https://github.com/wojciechkepka/pkger/pull/58)
- Add `--no-sign` command line argument to `build` command so that users can disable signing for the particular build
  even when the gpg signing is enabled in the configuration.
- Move `forward_ssh_agent` option to `ssh` configuration and rename it to `forward_agent` [#60](https://github.com/wojciechkepka/pkger/pull/60)
- Add `disable_key_verification` to `ssh` configuration [#60](https://github.com/wojciechkepka/pkger/pull/60)
- Add a warning when trying to build a recipe without a target
- Fix applying patches with relative paths [#64](https://github.com/wojciechkepka/pkger/pull/64)
- Recipes can now have a `recipe.yml` or `recipe.yaml` file where previously only `.yml` was accepted
  [#64](https://github.com/wojciechkepka/pkger/pull/64)

# 0.3.0
- Configure script now has a working directory set to `$PKGER_BLD_DIR`
- Recipes and config files now use YAML syntax [#52](https://github.com/wojciechkepka/pkger/pull/52)
- Add extra field to metadata that specifies image os if pkger fails to find it out
- Add ability to apply patches to source based on target image
- Directory structure of `output_dir` changed, now all images have a separate directory with output packages
- Fix ubuntu builds
- Add `list < recipes | images >` subcommand

# 0.2.1
- Fix setting default working directory in build and install phase

# 0.2.0

- pkger doesn't start a build by default, there is a separate subcommand `build` for that now. [#22](https://github.com/wojciechkepka/pkger/pull/22)
- Add `gen-recipe` subcommand to declaratively generate recipes [#22](https://github.com/wojciechkepka/pkger/pull/22)
- Build and install scripts now correctly have a working directory set [#23](https://github.com/wojciechkepka/pkger/pull/23)
- Allow overwriting default working directory of each script phase [#24](https://github.com/wojciechkepka/pkger/pull/24)
- Add option to change default shell in recipe scripts [#26](https://github.com/wojciechkepka/pkger/pull/26)
- Excluding paths from final package now works [#36](https://github.com/wojciechkepka/pkger/pull/36)
- Actually check if image should be rebuilt in docker [#37](https://github.com/wojciechkepka/pkger/pull/37)
- Cache images with dependencies installed, a lot of data saved on pulled dependencies [#38](https://github.com/wojciechkepka/pkger/pull/38)
- Dependencies now use the TOML syntax instead of a custom one [#39](https://github.com/wojciechkepka/pkger/pull/39)
- Commands in configure, build and install scripts now use TOML syntax [#40](https://github.com/wojciechkepka/pkger/pull/40)
- Add `--trace` option that sets log level of pkger to trace and make `--debug` actually set debug
- Add some more fields for RPM builds, rename `section` to `group` and use it for RPM as well as DEB [#41](https://github.com/wojciechkepka/pkger/pull/41)
- Separate RPM and DEB fields in recipe metadata [#42](https://github.com/wojciechkepka/pkger/pull/42)
- Add missing fields for DEB builds, add `url` field to metadata [#43](https://github.com/wojciechkepka/pkger/pull/43)
- Add initial PKG target [#44](https://github.com/wojciechkepka/pkger/pull/44)
- Make `release` and `epoch` fields of metadata global rather than RPM specific
- Add some missing extra fields in metadata for PKG [#45](https://github.com/wojciechkepka/pkger/pull/45)
- Add optional boolean flags in recipe scripts that specify on which targets each command should be run
- Add a simple oneshot build without custom images [#46](https://github.com/wojciechkepka/pkger/pull/46)
