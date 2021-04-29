# Generate recipes

To generate a recipe declaratively from CLI use subcommand `gen-recipe`. By default it requires only the name of the package and prints the recipe to stdout. If `output_dir` is provided **pkger** will create a directory with the name of the package and a `recipe.yml` containing generated recipe.

Example generated recipe with no options printed to stdout:
```
> pkger gen-recipe blank
[metadata]
name = "blank"
version = "1.0.0"
description = "missing"
license = "missing"
images = []

[metadata.deb]

[metadata.rpm]

[metadata.pkg]

[build]
steps = []
```

Or a more complex one, all of the metadata fields can be added using declarative syntax:
```
> pkger gen-recipe test --arch x86_64 --description "A very interesting package..." \
                        --provides test-bin --version 1.0.0 --build-depends curl make \
                        --license MIT
[metadata]
name = "test"
version = "1.0.0"
description = "A very interesting package..."
license = "MIT"
images = []
arch = "x86_64"
build_depends = ["curl", "make"]
provides = ["test-bin"]

[metadata.deb]

[metadata.rpm]

[metadata.pkg]

[build]
steps = []
```
