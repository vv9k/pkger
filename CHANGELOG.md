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
