# Signing
To sign packages automatically using a GPG key add the following to your [configuration file](./configuration.md):

```yaml
gpg_key: /absolute/path/to/the/private/key
gpg_name: Packager Name # must be the same as the `Name` field on the key
```

When **pkger** detects the gpg key in the configuration it will prompt for a password to the key on each run.

Currently, only *deb* and *rpm* targets support signing.

