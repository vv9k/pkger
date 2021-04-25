# Config

Config file has a following structure:

```toml
# required
recipes_dir = ""
output_dir = ""

# optional
images_dir = ""
docker = "unix:///var/run/docker.sock"
```
`images_dir` - directory with images
  - Each image is a directory containing a `Dockerfile` and files to be imported with it
  - Image name is the directory name

`recipes_dir` - directory with recipes
  - Each recipe is a directory containing a `recipe.toml` file and source files (if not remote) 

`output_dir` - directory with built packages
  - When **pkger** finishes building the package it will create a directory `$output_dir/$PKGER_OS/$PKGER_OS_VERSION/` where it will put the built package

`docker` - specify docker uri in configuration.

If an option is available as both configuration parameter and cli argument **pkger** will favour the arguments passed during startup.


By default **pkger** will look for the config file in the home directory of the user running the process in a file `.pkger.toml`. If there is no global configuration current directory will be scand for the same file. To specify the location of the config file use `--config` or `-c` parameter.
