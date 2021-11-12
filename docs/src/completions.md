# Shell completions

**pkger** provides a subcommand to print shell completions. Supported shells are: *bash*, *zsh*, *fish*, *powershell*, *elvish*.

To print the completions run:
```shell
pkger print-completions bash
```

replacing `bash` with whatever shell you prefer.


To have completions automatically add something along those lines to your `.bashrc`, `.zshrc`...:
```shell
. <(pkger print-completions bash)
```
