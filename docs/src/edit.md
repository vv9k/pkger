# Edit recipes, images and config

**pkger** provides utility subcommand `edit` that invokes the default editor defined by `$EDITOR` environment variable.
To make this functionality work, export this variable in your shell's init script like `~/.bashrc`.

Edit images and recipes by name:

```
# This will open up the Dockerfile in the `centos8` image.
$ pkger edit image centos8 

# This will open up the `recipe.yml` or `recipe.yaml` file in `pkger-simple` recipe directory
$ pkger edit recipe pkger-simple

```


To edit the configuration file run:
```
$ pkger edit config
```
