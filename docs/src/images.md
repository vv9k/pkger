# Images

Images are an optional feature of **pkger**.By default **pkger** will create necessary images to build simple targets,
they are completely distinct from user defined ones. User images offer higher customisation when it comes to preparing
the build environment.

In the images directory specified by the [configuration](./configuration.md) **pkger** will treat each subdirectory
containing a `Dockerfile` as an image. The name of the directory will become the name of the image.

So example structure like this:
```
images
├── arch
│  └── Dockerfile
├── rocky
│  └── Dockerfile
└── debian
   └── Dockerfile
```
**pkger** will detect 3 images - *arch*, *rocky* and *debian*.

Images with dependencies installed will be cached for each recipe-target combo to reduce the number of times the
dependencies have to be pulled from remote sources. This saves a lot of space, time and bandwith.


You can declare a new image with a subcommand. It will automatically create a directory in `images_dir`
containing an empty `Dockerfile`.

```shell
$ pkger new image <NAME>
```

There is also a way to remove images. The `remove` subcommand erases the whole directory of an image if
such exists:

```shell
$ pkger remove images <NAMES>...

# or shorhand 'rm' for 'remove' and 'img' for 'images'
$ pkger rm img <NAMES>...
```

To see existing images use:
```shell
$ pkger list images

# or shorhand 'ls' for 'list' and 'img' for 'images'
$ pkger ls img

# for more detailed output
$ pkger list -v images
```

