# DEB fields

Optional fields that may be used when building a DEB package.

```yaml
  deb:
    priority: ""
    built_using: ""
    essential: true
    
    # specify the content of post install script
    postinst: ""

    # same as all other dependencies but deb specific
    pre_depends: []
    recommends: []
    suggests: []
    breaks: []
    replaces: []
    enhances: []
```
