# Formatting output

By default **pkger** will display basic output as hierhical log with level set to `INFO`. To debug run with `-d` or `--debug` option. To surpress all output except for errors add `-q` or `--quiet`. To manually set log level set `RUST_LOG` env variable to a value like `pkger=debug` with debug replaced with the desired log level.

To decide what parts of events are displayed use the `--hide` flag that takes a filter string as input and tells **pkger** what fields to display. Each character of filter string is responsible for a single part of output. Characters are case insensitive, the order doesn't matter and duplicates are silently ignored. Available modules are:
 - `d` hides the timestamp
 - `f` hides the fields in spans (the values between curly braces like `{id = vw89wje92}`)
 - `l` hides the level
 - `s` hides the spans entirely
