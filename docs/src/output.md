# Formatting output

By default **pkger** will display basic output as hierhical log with level set to `INFO`. All log messages will be printed to stdout unless a `--log-dir` flag (or `log_dir` is specified in [configuration](./configuration.md)) is provided, in that case there will be a single global log file in the logging directory created on each run as well as a separate file for each task.

To debug run with `-d` or `--debug` option. To surpress all output except for errors and warnings add `-q` or `--quiet`. To enable very verbose output add `-t` or `--trace option.
