# Shell completions and man page

```sh
ndac completions bash > ~/.local/share/bash-completion/completions/ndac
ndac completions zsh  > "${fpath[1]}/_ndac"
ndac completions fish > ~/.config/fish/completions/ndac.fish
ndac man > ndac.1 && man ./ndac.1
```

`completions` supports `bash`, `zsh`, `fish`, `elvish` and `powershell`.
Release archives and packages include both, generated the same way.
