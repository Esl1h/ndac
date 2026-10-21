# nextform

NextDNS profiles as code. `nextform` keeps the desired state of your NextDNS
profiles in TOML files, reads the live state from the NextDNS API, and shows
the difference as a plan you can review and apply, in the style of Terraform
and octoDNS but without a state file.

> **Status:** early development. Nothing is usable yet; the repository
> currently contains the workspace skeleton, policies and CI. The command line
> interface, configuration format and crate APIs will change without notice
> until the first release.

## Planned workflow

```sh
nextform init          # scaffold a config repository
nextform import --all  # write live profiles to TOML
nextform plan          # review changes and drift
nextform apply         # apply them, with a snapshot taken first
```

## Security

The NextDNS API key grants access to the whole account, including DNS logs.
`nextform` never accepts the key as a command-line argument; it resolves it
from the environment, a file, the OS keyring, a password manager command, or
systemd credentials. See [SECURITY.md](SECURITY.md) to report a vulnerability.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and the
[Code of Conduct](CODE_OF_CONDUCT.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
