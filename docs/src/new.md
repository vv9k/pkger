# Generate recipes

To generate a recipe declaratively from CLI use the `pkger new recipe` subcommand. By default it requires only the name
of the  package and creates a directory with `recipe.yml` in it.


# Create images

To create images use `pkger new image <name>`. This will create a directory with a `Dockerfile` in the `images_dir`
specified in the [configuration](./configuration.md).
