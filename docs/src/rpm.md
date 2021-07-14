# RPM fields

Optional fields that will be used when building RPM target.

```yaml
  rpm:
    vendor: ""
    icon: ""
    summary: "shorter description" # if not provided defaults to value of `description`
    config_noreplace: "%{_sysconfdir}/%{name}/%{name}.conf"

    pre_script: ""
    post_script: ""
    preun_script: ""
    postun_script: ""

    # acts the same as other dependencies - can be passed as array
    #obsoletes: ["foo"]
    # or as a map
    obsoletes:
      centos8: ["foo"]
```

## Signing
To sign packages using a GPG key add the following to your [configuration file](./configuration.md):

```yaml
gpg_key: /absolute/path/to/the/private/key
gpg_name: Packager Name # must be the same as the `Name` field on the key
```

When **pkger** detects the gpg key in the configuration it will prompt for a password to the key on each run.

