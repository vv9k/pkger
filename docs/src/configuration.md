# Configuration

By default **pkger** will look for the config file in the home directory of the user running the process in a file
`.pkger.yml`. If there is no global configuration current directory will be scand for the same file. To specify the
location of the config file use `--config` or `-c` parameter.

The configuration file has a following structure:

```yaml
# required
recipes_dir: ""
output_dir: ""

# optional
images_dir: ""
docker: "unix:///var/run/docker.sock"

ssh:
  # this will make the ssh auth socket available to the container so that it can use private keys from the host.
  forward_agent: true

  # This will allow tools that use SSH to connect to hosts that are not present in the `authorized_keys`
  disable_key_verification: true
```

The required fields when running a build are `recipes_dir` and `output_dir`. First tells **pkger** where to look for
[recipes](./recipes.md) to build, the second is the directory where the final packages will end up.

When using [custom images](./images.md) their location can be specified with `images_dir`.

If Docker daemon that **pkger** should connect does not run on a default unix socket override the uri with `docker` parameter.

If an option is available as both configuration parameter and cli argument **pkger** will favour the arguments passed during startup.

