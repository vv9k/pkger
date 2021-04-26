# DEB fields

Optional fields that may be used when building a DEB package.

```toml
[metadata.deb]
priority = ""
installed_size = ""
built_using = ""
essential = true

# same as all other dependencies but deb specific
pre_depends = []
recommends = []
suggests = []
breaks = []
replaces = []
enchances = []
```
