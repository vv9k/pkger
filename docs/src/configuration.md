# Configuration

By default **pkger** will look for the config file named `.pkger.yml` in the config directory appropriate for the OS 
that **pkger** is run on. If there is no global configuration, current directory will be scanned for the same file. 
To specify the location of the config file use `--config` or `-c` parameter.

The configuration file has a following structure:

```yaml
# required
recipes_dir: ""
output_dir: ""

# optional
images_dir: ""
docker: "unix:///var/run/docker.sock"

# A formatting filter that decides what gets displayed with each output message. This acts the same as CLI argument
# `--filter`.
# All characters can be upper or lower case, the order doesn't matter, duplicates and errors are silently ignored.
# Available fields to show are: D - Date, F - Fields, S - Spans.
# Available fields to hide are: L - Levels.
filter: "SFL" # will display spans and fields of the spans but the level like `INFO` will be omitted

ssh:
  # this will make the ssh auth socket available to the container so that it can use private keys from the host.
  forward_agent: true

  # This will allow tools that use SSH to connect to hosts that are not present in the `known_hosts` file
  disable_key_verification: true


# To define custom images add the following
images:
  - name: centos8
    target: rpm
  - name: debian10
    target: deb
# if pkger fails to find out the operating system you can specify it by os parameter
  - name: arch
    target: pkg
    os: Arch Linux
```

The required fields when running a build are `recipes_dir` and `output_dir`. First tells **pkger** where to look for
[recipes](./recipes.md) to build, the second is the directory where the final packages will end up.

When using [custom images](./images.md) their location can be specified with `images_dir`.

If Docker daemon that **pkger** should connect does not run on a default unix socket override the uri with `docker`
parameter.

If an option is available as both configuration parameter and cli argument **pkger** will favour the arguments passed
during startup.


## Generate configuration file and directories

To quickly start of with **pkger** use the `pkger init` subcommand that will create necessary directories and the
configuration file. Default locations can be overridden by command line parameters.