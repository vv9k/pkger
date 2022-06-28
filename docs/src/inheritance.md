# Recipes inheritance


Recipes support inheriting fields from a defined base recipe to avoid repetition. For example here is a definition of a base package:

```yaml
---
metadata:
  name: base-package
  version: 0.1.0
  description: pkger base package testing recipe inheritance
  arch: x86_64
  license: MIT
  images: [ rocky, debian ]
build:
  working_dir: $PKGER_OUT_DIR
  steps:
    - cmd: echo 123 >> ${RECIPE}_${RECIPE_VERSION}
```

And here is a child recipe using `from` field to define the parent recipe:

```yaml
---
from: base-package
metadata:
  name: child-package1
  version: 0.2.0
  description: pkger child package testing recipe inheritance from base-package
install:
  shell: /bin/bash
  steps:
    - cmd: >-
        if [[ $(cat ${RECIPE}_${RECIPE_VERSION}) =~ 123 ]]; then exit 0; else
        echo "Test file ${RECIPE}_${RECIPE_VERSION} has invalid content"; exit 1; fi
```


The `child-package1` will share the `build` steps as well as `arch`, `license`, `images` fields. After merging the child recipe will look something like this:

```yaml
---
from: base-package
metadata:
  name: child-package1
  version: 0.2.0
  description: pkger child package testing recipe inheritance from base-package
  arch: x86_64
  license: MIT
  images: [ rocky, debian ]
build:
  working_dir: $PKGER_OUT_DIR
  steps:
    - cmd: echo 123 >> ${RECIPE}_${RECIPE_VERSION}
install:
  shell: /bin/bash
  steps:
    - cmd: >-
        if [[ $(cat ${RECIPE}_${RECIPE_VERSION}) =~ 123 ]]; then exit 0; else
        echo "Test file ${RECIPE}_${RECIPE_VERSION} has invalid content"; exit 1; fi
```


When defining a child recipe only `from` and `metadata.name` fields are required. Here is a minimal child recipe:

```yaml
---
from: base-package
metadata:
  name: child-package2
```

For a working example refer to the [`example` directory](https://github.com/vv9k/pkger/tree/master/example) of **pkger** source tree.
